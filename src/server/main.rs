use std::error::Error;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};

use async_channel::Receiver;
use simple_logger::SimpleLogger;
use tokio::net::{TcpStream, UdpSocket};
use trust_dns_server::ServerFuture;

use common::io::{AsyncReader, AsyncReadWrapper, AsyncWriter, AsyncWriteWrapper, copy, Stream};
use dns::EchoRequestHandler;

use crate::tunnel::{TcpUntunneler, Untunneler};

mod tunnel;
mod dns;

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
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8899));
    let udp_socket = UdpSocket::bind(addr).await?;
    let mut server = ServerFuture::new(EchoRequestHandler {});
    server.register_socket(udp_socket);
    match server.block_until_done().await {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn forward_clients(
    clients: Receiver<Stream>,
    remote_address: IpAddr,
    remote_port: u16,
) -> Result<(), Box<dyn Error>> {
    while let Ok(mut client) = clients.recv().await {
        let to_address = format!("{}:{}", remote_address, remote_port);
        log::debug!("connecting to {}", to_address);
        let stream = TcpStream::connect(to_address).await?;
        let (to_reader, to_writer) = stream.into_split();
        let mut to_tunnel: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(to_reader));
        let mut from_tunnel: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(to_writer));
        let to_tunnel_future = copy(&mut to_tunnel, &mut client.writer, "sending to tunnel");
        let from_tunnel_future = copy(&mut client.reader, &mut from_tunnel, "received from tunnel");
        tokio::try_join!(to_tunnel_future, from_tunnel_future)?;
    }
    Ok(())
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
