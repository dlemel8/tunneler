use std::error::Error;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_channel::Receiver;
use simple_logger::SimpleLogger;
use structopt::StructOpt;

use common::cli::{Cli, TunneledType, TunnelerType};
use common::io::Stream;

use crate::dns::DnsUntunneler;
use crate::network::{forward_client_tcp, forward_client_udp};
use crate::tunnel::{TcpUntunneler, Untunneler};

mod dns;
mod io;
mod network;
mod tunnel;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Cli = Cli::from_args();

    SimpleLogger::new()
        .with_level(args.log_level)
        .init()
        .unwrap();
    log::debug!("start server - args are {:?}", args);

    start_server(args)
}

#[tokio::main]
async fn start_server(args: Cli) -> Result<(), Box<dyn Error>> {
    let mut untunneler =
        new_untunneler(args.tunneler_type, args.local_address, args.local_port).await?;
    let (untunneled_sender, untunneled_receiver) = async_channel::unbounded::<Stream>();

    let untunnel_clients_future = untunneler.untunnel(untunneled_sender);
    let forward_clients_future = forward_clients(
        untunneled_receiver,
        Arc::new(args.tunneled_type),
        args.remote_address,
        args.remote_port,
    );
    match tokio::try_join!(untunnel_clients_future, forward_clients_future) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn new_untunneler(
    type_: TunnelerType,
    address: IpAddr,
    port: u16,
) -> Result<Box<dyn Untunneler>, Box<dyn Error>> {
    match type_ {
        TunnelerType::Tcp => Ok(Box::new(TcpUntunneler::new(address, port).await?)),
        TunnelerType::Dns {
            read_timeout_in_milliseconds,
            idle_client_timeout_in_milliseconds,
            client_suffix,
        } => Ok(Box::new(
            DnsUntunneler::new(
                address,
                port,
                Duration::from_millis(read_timeout_in_milliseconds),
                Duration::from_millis(idle_client_timeout_in_milliseconds),
                client_suffix,
            )
            .await?,
        )),
    }
}

async fn forward_clients(
    clients: Receiver<Stream>,
    type_: Arc<TunneledType>,
    remote_address: IpAddr,
    remote_port: u16,
) -> Result<(), Box<dyn Error>> {
    while let Ok(client) = clients.recv().await {
        tokio::spawn(forward_client(
            client,
            type_.clone(),
            remote_address,
            remote_port,
        ));
    }
    Ok(())
}

async fn forward_client(
    client: Stream,
    type_: Arc<TunneledType>,
    remote_address: IpAddr,
    remote_port: u16,
) {
    match *type_ {
        TunneledType::Tcp => forward_client_tcp(client, remote_address, remote_port).await,
        TunneledType::Udp => forward_client_udp(client, remote_address, remote_port).await,
    };
}
