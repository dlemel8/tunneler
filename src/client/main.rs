use std::error::Error;
use std::net::IpAddr;

use simple_logger::SimpleLogger;
use structopt::{clap::arg_enum, StructOpt};

use common::io::{Stream, TcpServer};

use crate::dns::DnsTunneler;
use crate::tunnel::{TcpTunneler, Tunneler};
use async_channel::Receiver;

mod dns;
mod tunnel;

arg_enum! {
    #[derive(Debug, Copy, Clone)]
    enum TunnelType {
        Tcp,
        Dns,
    }
}

#[derive(StructOpt, Debug)]
struct Cli {
    #[structopt(env, possible_values = & TunnelType::variants(), case_insensitive = true)]
    tunnel_type: TunnelType,

    #[structopt(env)]
    remote_address: IpAddr,

    #[structopt(env)]
    remote_port: u16,

    #[structopt(default_value = "0.0.0.0", long, env)]
    local_address: IpAddr,

    #[structopt(default_value = "8888", long, env)]
    local_port: u16,

    #[structopt(default_value = "info", long, env)]
    log_level: log::LevelFilter,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Cli = Cli::from_args();
    SimpleLogger::new()
        .with_level(args.log_level)
        .init()
        .unwrap();
    log::debug!("args are {:?}", args);

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

pub(crate) async fn new_tunneler(
    tunnel_type: TunnelType,
    address: IpAddr,
    port: u16,
) -> Result<Box<dyn Tunneler>, Box<dyn Error>> {
    match tunnel_type {
        TunnelType::Tcp => Ok(Box::new(TcpTunneler::new(address, port).await?)),
        TunnelType::Dns => Ok(Box::new(DnsTunneler::new(address, port).await?)),
    }
}

async fn tunnel_clients(
    clients: Receiver<Stream>,
    tunnel_type: TunnelType,
    remote_address: IpAddr,
    remote_port: u16,
) -> Result<(), Box<dyn Error>> {
    while let Ok(client) = clients.recv().await {
        let mut tunnel = new_tunneler(tunnel_type, remote_address, remote_port).await?;
        tunnel.tunnel(client.reader, client.writer).await?
    }
    Ok(())
}
