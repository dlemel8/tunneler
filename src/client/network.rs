use std::convert::TryFrom;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::{duplex, split, AsyncReadExt, AsyncWriteExt, DuplexStream, ReadHalf};
use tokio::net;
use tokio::net::UdpSocket;

use common::io::Stream;
use common::network::Listener;

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

const STREAMED_UDP_PACKET_HEADER_SIZE: usize = 2;

#[async_trait]
impl Listener for UdpListener {
    async fn accept_clients(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        let mut data = vec![0; u16::MAX as usize];
        while let Ok((size, client_address)) = self.socket.recv_from(&mut data).await {
            log::debug!("got connection from {}", client_address);

            let (local, remote) = duplex(4096);
            let (remote_reader, remote_writer) = split(remote);
            new_clients
                .send(Stream::new(remote_reader, remote_writer))
                .await?;

            let (local_reader, mut local_writer) = split(local);
            let size_u16 = u16::try_from(size)?;
            local_writer.write(&size_u16.to_be_bytes()).await?;
            local_writer.write(&data[..size]).await?;
            tokio::spawn(unstream_udp(
                local_reader,
                self.socket.clone(),
                client_address,
            ));
        }
        Ok(())
    }
}

async fn unstream_udp(
    mut reader: ReadHalf<DuplexStream>,
    socket: Arc<UdpSocket>,
    target: SocketAddr,
) {
    while let Some(data) = read_streamed_data(&mut reader).await {
        log::debug!("sending to {} received data {:?}", target, data);
        if let Err(e) = socket.send_to(&data, &target).await {
            log::error!("failed to sending  data to {}: {}", target, e);
        };
    }
}

async fn read_streamed_data(reader: &mut ReadHalf<DuplexStream>) -> Option<Vec<u8>> {
    let mut header_bytes = [0; STREAMED_UDP_PACKET_HEADER_SIZE];
    let header_size = match reader.read_exact(&mut header_bytes).await {
        Ok(size) => size,
        Err(e) => {
            log::error!("failed to read header: {}", e);
            return None;
        }
    };

    if header_size != STREAMED_UDP_PACKET_HEADER_SIZE {
        log::error!("got unexpected header size in bytes {}", header_size);
        return None;
    }

    let header = u16::from_be_bytes(header_bytes);
    let header_usize = header as usize;
    let mut data = vec![0; header_usize];
    let size = match reader.read_exact(&mut data).await {
        Ok(size) => size,
        Err(e) => {
            log::error!("failed to read data: {}", e);
            return None;
        }
    };

    if size != header_usize {
        log::error!("got unexpected data size in bytes {}", header_size);
        return None;
    }

    Some(data)
}
