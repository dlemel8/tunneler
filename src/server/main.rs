use std::error::Error;
use std::net::IpAddr;
use std::time::Duration;

use async_channel::Receiver;
use simple_logger::SimpleLogger;
use structopt::StructOpt;
use tokio::net::TcpStream;

use common::cli::{Cli, TunnelType};
use common::io::{copy, Stream};

use crate::dns::DnsUntunneler;
use crate::tunnel::{TcpUntunneler, Untunneler};

mod dns;
mod io;
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
        new_untunneler(args.tunnel_type, args.local_address, args.local_port).await?;
    let (untunneled_sender, untunneled_receiver) = async_channel::unbounded::<Stream>();

    let untunnel_clients_future = untunneler.untunnel(untunneled_sender);
    let forward_clients_future =
        forward_clients(untunneled_receiver, args.remote_address, args.remote_port);
    match tokio::try_join!(untunnel_clients_future, forward_clients_future) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn new_untunneler(
    tunnel_type: TunnelType,
    address: IpAddr,
    port: u16,
) -> Result<Box<dyn Untunneler>, Box<dyn Error>> {
    match tunnel_type {
        TunnelType::Tcp => Ok(Box::new(TcpUntunneler::new(address, port).await?)),
        TunnelType::Dns {
            read_timeout_in_milliseconds,
            idle_client_timeout_in_milliseconds,
        } => Ok(Box::new(
            DnsUntunneler::new(
                address,
                port,
                Duration::from_millis(read_timeout_in_milliseconds),
                Duration::from_millis(idle_client_timeout_in_milliseconds),
            )
            .await?,
        )),
    }
}

async fn forward_clients(
    clients: Receiver<Stream>,
    remote_address: IpAddr,
    remote_port: u16,
) -> Result<(), Box<dyn Error>> {
    while let Ok(client) = clients.recv().await {
        tokio::spawn(forward_client(client, remote_address, remote_port));
    }
    Ok(())
}

async fn forward_client(mut client: Stream, remote_address: IpAddr, remote_port: u16) {
    let to_address = format!("{}:{}", remote_address, remote_port);
    log::debug!("connecting to {}", to_address);
    let server = match TcpStream::connect(&to_address).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("failed to connect to {}: {}", to_address, e);
            return;
        }
    };

    let (server_reader, server_writer) = server.into_split();
    let mut server_stream = Stream::new(server_reader, server_writer);
    let to_tunnel_future = copy(
        &mut server_stream.reader,
        &mut client.writer,
        "sending to tunnel",
    );
    let from_tunnel_future = copy(
        &mut client.reader,
        &mut server_stream.writer,
        "received from tunnel",
    );
    if let Err(e) = tokio::try_join!(to_tunnel_future, from_tunnel_future) {
        log::error!("failed to forward client: {}", e)
    }
}
