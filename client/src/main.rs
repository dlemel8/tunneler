use std::error::Error;
use std::net::IpAddr;

use simple_logger::SimpleLogger;
use structopt::StructOpt;
use tokio::net::{TcpListener, TcpStream};

use crate::tunnel::{AsyncReadWrapper, AsyncWriteWrapper, Tunnel, TcpTunnel};
use tokio::io::AsyncWriteExt;
use crate::dns::DnsTunnel;

mod dns;
mod tunnel;

#[derive(StructOpt, Debug)]
struct Cli {
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
        let remote_address = args.remote_address;
        let remote_port = args.remote_port;
        let task = async move {
            if let Err(e) = handle_in_stream(remote_address, remote_port, in_stream).await {
                log::error!("failed to handle in stream: {}", e);
            }
        };

        tokio::spawn(task);
    }

    Ok(())
}

pub(crate) async fn new_tunnel(address: IpAddr, port: u16) -> Result<Box<dyn Tunnel>, Box<dyn Error>> {
    let tunnel = TcpTunnel::new(address, port).await?;
    // let tunnel = DnsTunnel::new(address, port).await?;
    return Ok(Box::new(tunnel));
}

async fn handle_in_stream(
    remote_address: IpAddr,
    remote_port: u16,
    in_stream: TcpStream,
) -> Result<(), Box<dyn Error>> {
    let (in_read, mut in_write) = in_stream.into_split(); // TODO: convert to split, and use reference instead of box?

    let mut tunnel = new_tunnel(remote_address, remote_port).await?;
    let reader = Box::new(AsyncReadWrapper { reader: in_read });
    let writer = Box::new(AsyncWriteWrapper { writer: in_write });
    match tunnel.tunnel(reader, writer).await {
        Ok(_) => {
            // TODO: restore this
            // in_write.shutdown().await;
            Ok(())
        },
        Err(e) => Err(e.into()),
    }
}
