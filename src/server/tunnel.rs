use std::error::Error;
use std::net::IpAddr;

use async_channel::{Receiver, Sender};
use async_trait::async_trait;

use common::io::Stream;
use common::network::{Listener, TcpListener};

#[async_trait(? Send)]
pub(crate) trait Untunneler {
    async fn untunnel(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>>;
}

pub(crate) struct TcpUntunneler<L: Listener> {
    listener: L,
}

impl TcpUntunneler<TcpListener> {
    pub(crate) async fn new(
        local_address: IpAddr,
        local_port: u16,
    ) -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::new(local_address, local_port).await?;
        Ok(TcpUntunneler::from_listener(listener))
    }
}

impl<L: Listener> TcpUntunneler<L> {
    pub(crate) fn from_listener(listener: L) -> Self {
        Self { listener }
    }
}

#[async_trait(? Send)]
impl<L: Listener> Untunneler for TcpUntunneler<L> {
    async fn untunnel(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        let (tunneled_sender, tunneled_receiver) = async_channel::unbounded::<Stream>();
        let accept_clients_future = self.listener.accept_clients(tunneled_sender);
        let untunnel_clients_future = untunnel_clients(tunneled_receiver, new_clients);
        match tokio::try_join!(untunnel_clients_future, accept_clients_future) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

async fn untunnel_clients(
    tunneled: Receiver<Stream>,
    untunneled: Sender<Stream>,
) -> Result<(), Box<dyn Error>> {
    while let Ok(tunneled_client) = tunneled.recv().await {
        untunneled.send(tunneled_client).await?;
    }
    Ok(())
}
