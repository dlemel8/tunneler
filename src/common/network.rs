use std::convert::TryFrom;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::net;
use tokio::net::UdpSocket;

use crate::io::{AsyncReader, AsyncWriter, Stream};

#[async_trait]
pub trait Listener {
    async fn accept_clients(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>>;
}

pub struct TcpListener {
    listener: net::TcpListener,
}

impl TcpListener {
    pub async fn new(local_address: IpAddr, local_port: u16) -> Result<Self, Box<dyn Error>> {
        let listener_address = format!("{}:{}", local_address, local_port);
        log::info!("start listening on {}", listener_address);
        let listener = net::TcpListener::bind(listener_address).await?;
        Ok(Self { listener })
    }
}

#[async_trait]
impl Listener for TcpListener {
    async fn accept_clients(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        while let Ok((client_stream, client_address)) = self.listener.accept().await {
            log::debug!("got connection from {}", client_address);
            let (client_reader, client_writer) = client_stream.into_split();
            new_clients
                .send(Stream::new(client_reader, client_writer))
                .await?;
        }
        Ok(())
    }
}

pub const MAX_UDP_PACKET_SIZE: usize = u16::MAX as usize;
pub const STREAMED_UDP_PACKET_HEADER_SIZE: usize = 2;

pub async fn stream_udp_packet(
    payload: &mut Vec<u8>,
    size: usize,
    writer: &mut Box<dyn AsyncWriter>,
) {
    if payload.len() < size {
        log::error!(
            "payload {:?} is too small (expecting size {})",
            payload,
            size
        );
        return;
    }

    let size_u16 = match u16::try_from(size) {
        Ok(s) => s,
        Err(e) => {
            log::error!("size {} can't fit in a u16: {}", size, e);
            return;
        }
    };

    if let Err(e) = writer.write(&size_u16.to_be_bytes()).await {
        log::error!("failed to write header: {}", e);
        return;
    };

    if let Err(e) = writer.write(&payload[..size]).await {
        log::error!("failed to write payload: {}", e);
        return;
    };
}

pub async fn unstream_udp(
    mut reader: Box<dyn AsyncReader>,
    socket: Arc<UdpSocket>,
    target: SocketAddr,
) {
    while let Some(data) = unstream_udp_packet(&mut reader).await {
        log::debug!("sending to {} received data {:?}", target, data);
        if let Err(e) = socket.send_to(&data, &target).await {
            log::error!("failed to sending  data to {}: {}", target, e);
        };
    }
}

async fn unstream_udp_packet(reader: &mut Box<dyn AsyncReader>) -> Option<Vec<u8>> {
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
    let mut payload = vec![0; header_usize];
    let size = match reader.read_exact(&mut payload).await {
        Ok(size) => size,
        Err(e) => {
            log::error!("failed to read payload: {}", e);
            return None;
        }
    };

    if size != header_usize {
        log::error!("got unexpected data size in bytes {}", header_size);
        return None;
    }

    Some(payload)
}
