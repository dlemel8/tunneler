use std::error::Error;
use std::net::IpAddr;

use simple_logger::SimpleLogger;
use structopt::StructOpt;
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

#[derive(StructOpt, Debug)]
struct Cli {
    #[structopt(env)]
    remote_address: IpAddr,

    #[structopt(env)]
    remote_port: u16,

    #[structopt(default_value = "0.0.0.0", env = "ADDRESS")]
    local_address: IpAddr,

    #[structopt(default_value = "8888", env = "PORT")]
    local_port: u16,

    #[structopt(default_value = "info", env)]
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

    let to_address = format!("{}:{}", args.remote_address, args.remote_port);
    while let Ok((in_stream, in_address)) = listener.accept().await {
        log::debug!("got connection from {}", in_address);
        let to_address = to_address.clone();
        let task = async move {
            if let Err(e) = handle_in_stream(to_address, in_stream).await {
                log::error!("failed to handle in stream: {}", e);
            }
        };

        tokio::spawn(task);
    }

    Ok(())
}

async fn handle_in_stream(
    to_address: String,
    mut in_stream: TcpStream,
) -> Result<(), Box<dyn Error>> {
    log::debug!("connecting to {}", to_address);
    let mut out_stream = TcpStream::connect(to_address).await?;

    let (mut in_read, mut in_write) = in_stream.split();
    let (mut out_read, mut out_write) = out_stream.split();

    let in_to_out = async {
        io::copy(&mut in_read, &mut out_write).await?;
        out_write.shutdown().await
    };
    let out_to_in = async {
        io::copy(&mut out_read, &mut in_write).await?;
        in_write.shutdown().await
    };

    match tokio::try_join!(in_to_out, out_to_in) {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
