use std::error::Error;
use std::net::IpAddr;

use simple_logger::SimpleLogger;
use structopt::{clap::arg_enum, StructOpt};
use tokio::net::{TcpListener, TcpStream};

use crate::dns::DnsTunnel;
use crate::tunnel::{AsyncReadWrapper, AsyncWriteWrapper, TcpTunneler, Tunneler};

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

    let listener_address = format!("{}:{}", args.local_address, args.local_port);
    log::info!("start listening on {}", listener_address);
    // TODO - support both TCP and UDP?
    let listener = TcpListener::bind(listener_address).await?;

    while let Ok((in_stream, in_address)) = listener.accept().await {
        log::debug!("got connection from {}", in_address);
        let tunnel_type = args.tunnel_type;
        let remote_address = args.remote_address;
        let remote_port = args.remote_port;
        let task = async move {
            if let Err(e) =
                handle_in_stream(tunnel_type, remote_address, remote_port, in_stream).await
            {
                log::error!("failed to handle in stream: {}", e);
            }
        };

        tokio::spawn(task);
    }

    Ok(())
}

pub(crate) async fn new_tunneler(
    tunnel_type: TunnelType,
    address: IpAddr,
    port: u16,
) -> Result<Box<dyn Tunneler>, Box<dyn Error>> {
    match tunnel_type {
        TunnelType::Tcp => Ok(Box::new(TcpTunneler::new(address, port).await?)),
        TunnelType::Dns => Ok(Box::new(DnsTunnel::new(address, port).await?)),
    }
}

async fn handle_in_stream(
    tunnel_type: TunnelType,
    remote_address: IpAddr,
    remote_port: u16,
    in_stream: TcpStream,
) -> Result<(), Box<dyn Error>> {
    let (in_read, in_write) = in_stream.into_split();

    let mut tunnel = new_tunneler(tunnel_type, remote_address, remote_port).await?;
    let reader = Box::new(AsyncReadWrapper::new(in_read));
    let writer = Box::new(AsyncWriteWrapper::new(in_write));
    tunnel.tunnel(reader, writer).await
}
