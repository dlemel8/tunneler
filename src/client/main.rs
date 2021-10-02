use std::error::Error;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_channel::Receiver;
use simple_logger::SimpleLogger;
use structopt::StructOpt;

use common::cli::{Cli, TunneledType, TunnelerType};
use common::io::Stream;
use common::network::{Listener, TcpListener};

use crate::dns::DnsTunneler;
use crate::network::UdpListener;
use crate::tls::TlsTunneler;
use crate::tunnel::{TcpTunneler, Tunneler};

mod dns;
mod network;
mod tls;
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
async fn start_client(args: Cli) -> Result<(), Box<dyn Error>> {
    let mut listener =
        new_listener(args.tunneled_type, args.local_address, args.local_port).await?;
    let (clients_sender, clients_receiver) = async_channel::unbounded::<Stream>();

    let tunnel_type = Arc::new(args.tunneler_type);
    let accept_clients_future = listener.accept_clients(clients_sender);
    let tunnel_clients_future = tunnel_clients(
        clients_receiver,
        tunnel_type.clone(),
        args.remote_address,
        args.remote_port,
    );
    match tokio::try_join!(accept_clients_future, tunnel_clients_future) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

async fn new_listener(
    type_: TunneledType,
    address: IpAddr,
    port: u16,
) -> Result<Box<dyn Listener>, Box<dyn Error>> {
    match type_ {
        TunneledType::Tcp => Ok(Box::new(TcpListener::new(address, port).await?)),
        TunneledType::Udp => Ok(Box::new(UdpListener::new(address, port).await?)),
    }
}

async fn new_tunneler(
    type_: &TunnelerType,
    address: IpAddr,
    port: u16,
) -> Result<Box<dyn Tunneler>, Box<dyn Error>> {
    match type_ {
        TunnelerType::Tcp => Ok(Box::new(TcpTunneler::new(address, port).await?)),
        TunnelerType::Dns {
            read_timeout_in_milliseconds,
            idle_client_timeout_in_milliseconds,
            client_suffix,
        } => Ok(Box::new(
            DnsTunneler::new(
                address,
                port,
                Duration::from_millis(*read_timeout_in_milliseconds),
                Duration::from_millis(*idle_client_timeout_in_milliseconds),
                client_suffix.clone(),
            )
            .await?,
        )),
        TunnelerType::Tls {
            ca_cert,
            cert,
            key,
            server_hostname,
        } => Ok(Box::new(
            TlsTunneler::new(
                address,
                port,
                ca_cert.to_path_buf(),
                cert.to_path_buf(),
                key.to_path_buf(),
                server_hostname.clone(),
            )
            .await?,
        )),
    }
}

async fn tunnel_clients(
    clients: Receiver<Stream>,
    tunnel_type: Arc<TunnelerType>,
    remote_address: IpAddr,
    remote_port: u16,
) -> Result<(), Box<dyn Error>> {
    while let Ok(client) = clients.recv().await {
        tokio::spawn(tunnel_client(
            client,
            tunnel_type.clone(),
            remote_address,
            remote_port,
        ));
    }
    Ok(())
}

async fn tunnel_client(
    client: Stream,
    tunnel_type: Arc<TunnelerType>,
    remote_address: IpAddr,
    remote_port: u16,
) {
    let mut tunnel = match new_tunneler(&tunnel_type, remote_address, remote_port).await {
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
