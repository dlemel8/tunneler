use std::error::Error;
use std::net::IpAddr;

use async_channel::{Receiver, Sender};
use async_trait::async_trait;

use common::io::Stream;
use common::network::{Listener, TcpListener};

#[async_trait(?Send)]
pub(crate) trait Untunneler {
    async fn untunnel(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>>;
}

pub(crate) struct TcpUntunneler {
    server: TcpListener,
}

impl TcpUntunneler {
    pub(crate) async fn new(
        local_address: IpAddr,
        local_port: u16,
    ) -> Result<Self, Box<dyn Error>> {
        let server = TcpListener::new(local_address, local_port).await?;
        Ok(Self { server })
    }
}

#[async_trait(?Send)]
impl Untunneler for TcpUntunneler {
    async fn untunnel(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        let (tunneled_sender, tunneled_receiver) = async_channel::unbounded::<Stream>();
        let accept_clients_future = self.server.accept_clients(tunneled_sender);
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
