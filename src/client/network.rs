use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::{duplex, split};
use tokio::net;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

use common::cache::StreamsCache;
use common::io::Stream;
use common::network::{
    stream_udp_packet, unstream_udp_packet, Listener, UnstreamPacketResult, MAX_UDP_PACKET_SIZE,
    STREAMED_UDP_PACKET_HEADER_SIZE,
};

const CLIENT_IDLE_TIMEOUT: Duration = Duration::from_secs(60 * 3);
const CLIENT_READ_TIMEOUT: Duration = Duration::from_millis(100);

pub struct UdpListener {
    socket: Arc<net::UdpSocket>,
}

impl UdpListener {
    pub async fn new(local_address: IpAddr, local_port: u16) -> Result<Self, Box<dyn Error>> {
        let listener_address = format!("{}:{}", local_address, local_port);
        log::info!("start listening on {}", listener_address);
        let socket = net::UdpSocket::bind(listener_address).await?;
        Ok(Self {
            socket: Arc::new(socket),
        })
    }
}

#[async_trait]
impl Listener for UdpListener {
    async fn accept_clients(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        let cache = StreamsCache::with_default_cleanup_duration(
            move || {
                let (local, remote) = duplex(MAX_UDP_PACKET_SIZE + STREAMED_UDP_PACKET_HEADER_SIZE);

                let (remote_reader, remote_writer) = split(remote);
                new_clients.try_send(Stream::new(remote_reader, remote_writer))?;

                let (local_reader, local_writer) = split(local);
                Ok(Stream::new(local_reader, local_writer))
            },
            CLIENT_IDLE_TIMEOUT,
        );

        let mut data = vec![0; MAX_UDP_PACKET_SIZE];
        while let Ok((size, client_address)) = self.socket.recv_from(&mut data).await {
            log::debug!("got connection from {}", client_address);

            let client = cache.get(client_address, Instant::now())?;
            stream_udp(&data, size, &client).await;
            tokio::spawn(unstream_udp(
                client.clone(),
                self.socket.clone(),
                client_address,
            ));
        }
        Ok(())
    }
}

async fn stream_udp(data: &[u8], size: usize, client: &Arc<Mutex<Stream>>) {
    let writer = &mut client.lock().await.writer;
    stream_udp_packet(&data, size, writer).await;
}

async fn unstream_udp(client: Arc<Mutex<Stream>>, socket: Arc<net::UdpSocket>, target: SocketAddr) {
    loop {
        let reader = &mut client.lock().await.reader;
        let payload = match unstream_udp_packet(reader, Some(CLIENT_READ_TIMEOUT)).await {
            UnstreamPacketResult::Error => break,
            UnstreamPacketResult::Timeout => continue, // let stream_udp a chance to catch the lock
            UnstreamPacketResult::Payload(payload) => payload,
        };

        log::debug!("sending to {} received data {:?}", target, payload);
        if let Err(e) = socket.send_to(&payload, &target).await {
            log::error!("failed to sending  data to {}: {}", target, e);
        };
    }
}
