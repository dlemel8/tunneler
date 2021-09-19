use std::error::Error;
use std::net::IpAddr;
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::{duplex, split};
use tokio::net;

use common::io::{AsyncReadWrapper, AsyncReader, AsyncWriteWrapper, AsyncWriter, Stream};
use common::network::{
    stream_udp_packet, unstream_udp, Listener, MAX_UDP_PACKET_SIZE, STREAMED_UDP_PACKET_HEADER_SIZE,
};

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
        let mut data = vec![0; MAX_UDP_PACKET_SIZE];
        while let Ok((size, client_address)) = self.socket.recv_from(&mut data).await {
            log::debug!("got connection from {}", client_address);

            let (local, remote) = duplex(MAX_UDP_PACKET_SIZE + STREAMED_UDP_PACKET_HEADER_SIZE);
            let (remote_reader, remote_writer) = split(remote);
            new_clients
                .send(Stream::new(remote_reader, remote_writer))
                .await?;

            let (local_reader, local_writer) = split(local);
            let mut writer: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(local_writer));
            stream_udp_packet(&mut data, size, &mut writer).await;
            let reader: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(local_reader));
            tokio::spawn(unstream_udp(reader, self.socket.clone(), client_address));
        }
        Ok(())
    }
}
