use std::error::Error;
use std::net::IpAddr;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::net;

use crate::io::Stream;

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
