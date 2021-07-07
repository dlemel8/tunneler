use std::error::Error;
use std::net::IpAddr;

use async_channel::Receiver;
use simple_logger::SimpleLogger;
use tokio::net::TcpStream;

use common::io::{copy, AsyncReadWrapper, AsyncReader, AsyncWriteWrapper, AsyncWriter, Stream};

use crate::dns::DnsUntunneler;
use crate::tunnel::{TcpUntunneler, Untunneler};

mod dns;
mod io;
mod tunnel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()
        .unwrap();
    log::debug!("start server");

    dns_server().await?;
    tcp_server().await
}

async fn dns_server() -> Result<(), Box<dyn Error>> {
    let mut untunneler = DnsUntunneler::new("127.0.0.1".parse()?, 8899).await?;
    let (untunneled_sender, untunneled_receiver) = async_channel::unbounded::<Stream>();
    let untunnel_clients_future = untunneler.untunnel(untunneled_sender);
    let forward_clients_future = forward_clients(untunneled_receiver, "127.0.0.1".parse()?, 8080);
    match tokio::try_join!(untunnel_clients_future, forward_clients_future) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
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
    let stream = match TcpStream::connect(&to_address).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("failed to connect to {}: {}", to_address, e);
            return;
        }
    };

    let (to_reader, to_writer) = stream.into_split();
    let mut to_tunnel: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(to_reader));
    let mut from_tunnel: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(to_writer));
    let to_tunnel_future = copy(&mut to_tunnel, &mut client.writer, "sending to tunnel");
    let from_tunnel_future = copy(&mut client.reader, &mut from_tunnel, "received from tunnel");
    if let Err(e) = tokio::try_join!(to_tunnel_future, from_tunnel_future) {
        log::error!("failed to forward client: {}", e)
    }
}

async fn tcp_server() -> Result<(), Box<dyn Error>> {
    let mut untunneler = TcpUntunneler::new("127.0.0.1".parse()?, 8899).await?;
    let (untunneled_sender, untunneled_receiver) = async_channel::unbounded::<Stream>();
    let untunnel_clients_future = untunneler.untunnel(untunneled_sender);
    let forward_clients_future = forward_clients(untunneled_receiver, "127.0.0.1".parse()?, 8080);
    match tokio::try_join!(untunnel_clients_future, forward_clients_future) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}
