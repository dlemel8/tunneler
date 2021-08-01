use std::error::Error;
use std::net::IpAddr;
use std::time::Duration;

use async_channel::Receiver;
use simple_logger::SimpleLogger;
use structopt::StructOpt;

use common::cli::{Cli, TunnelType};
use common::io::{Stream, TcpServer};

use crate::dns::DnsTunneler;
use crate::tunnel::{TcpTunneler, Tunneler};

mod dns;
mod tunnel;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Cli = Cli::from_args();

    SimpleLogger::new()
        .with_level(args.log_level)
        .init()
        .unwrap();
    log::debug!("start client - args are {:?}", args);

    start_client(args)
}

#[tokio::main]
async fn start_client(args: Cli ) -> Result<(), Box<dyn Error>> {
    // TODO - support both TCP and UDP?
    let mut server = TcpServer::new(args.local_address, args.local_port).await?;
    let (clients_sender, clients_receiver) = async_channel::unbounded::<Stream>();

    let accept_clients_future = server.accept_clients(clients_sender);
    let tunnel_clients_future = tunnel_clients(
        clients_receiver,
        args.tunnel_type,
        args.remote_address,
        args.remote_port,
    );
    match tokio::try_join!(accept_clients_future, tunnel_clients_future) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn new_tunneler(
    tunnel_type: TunnelType,
    address: IpAddr,
    port: u16,
) -> Result<Box<dyn Tunneler>, Box<dyn Error>> {
    match tunnel_type {
        TunnelType::Tcp => Ok(Box::new(TcpTunneler::new(address, port).await?)),
        TunnelType::Dns {
            read_timeout_in_milliseconds,
            idle_client_timeout_in_milliseconds,
        } => Ok(Box::new(
            DnsTunneler::new(
                address,
                port,
                Duration::from_millis(read_timeout_in_milliseconds),
                Duration::from_millis(idle_client_timeout_in_milliseconds),
            )
            .await?,
        )),
    }
}

async fn tunnel_clients(
    clients: Receiver<Stream>,
    tunnel_type: TunnelType,
    remote_address: IpAddr,
    remote_port: u16,
) -> Result<(), Box<dyn Error>> {
    while let Ok(client) = clients.recv().await {
        tokio::spawn(tunnel_client(
            client,
            tunnel_type,
            remote_address,
            remote_port,
        ));
    }
    Ok(())
}

async fn tunnel_client(
    client: Stream,
    tunnel_type: TunnelType,
    remote_address: IpAddr,
    remote_port: u16,
) {
    let mut tunnel = match new_tunneler(tunnel_type, remote_address, remote_port).await {
        Ok(t) => t,
        Err(e) => {
            log::error!(
                "failed to create {:?} tunnel to {}:{}: {}",
                tunnel_type,
                remote_address,
                remote_port,
                e
            );
            return;
        }
    };
    if let Err(e) = tunnel.tunnel(client).await {
        log::error!("failed to tunnel client: {}", e);
    }
}
