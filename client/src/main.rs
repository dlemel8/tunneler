use std::error::Error;
use std::net::IpAddr;

use simple_logger::SimpleLogger;
use structopt::StructOpt;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::AsyncWriteExt;

#[derive(StructOpt, Debug)]
struct Cli {
    #[structopt()]
    remote_address: IpAddr,

    #[structopt()]
    remote_port: u16,

    #[structopt(default_value = "127.0.0.1", long = "local_address")]
    local_address: IpAddr,

    #[structopt(default_value = "8888", long = "local_port")]
    local_port: u16,

    #[structopt(default_value = "info", long = "log_level")]
    log_level: log::LevelFilter,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Cli = Cli::from_args();
    SimpleLogger::new().with_level(args.log_level).init().unwrap();
    log::debug!("args are {:?}", args);

    let listener_address = format!("{}:{}", args.local_address, args.local_port);
    log::info!("start listening on {}", listener_address);
    // TODO - support both TCP and UDP
    let listener = TcpListener::bind(listener_address).await?;

    let to_address = format!("{}:{}", args.remote_address, args.remote_port);
    while let (in_stream, in_address) = listener.accept().await? {
        log::debug!("got connection from {}", in_address);
        let task = handle_in_stream(to_address.clone(), in_stream);
        tokio::spawn(task);
    }

    Ok(())
}

async fn handle_in_stream(to_address: String, mut in_stream: TcpStream) -> Result<(), Box<dyn Error>> {
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

    tokio::try_join!(in_to_out, out_to_in);
    Ok(())
}
