use std::error::Error;
use std::net::IpAddr;

use async_trait::async_trait;
use tokio::net::TcpStream;

use common::io::{copy, Stream};

#[async_trait]
pub(crate) trait Tunneler: Send {
    async fn tunnel(&mut self, mut client: Stream) -> Result<(), Box<dyn Error>>;
}

pub(crate) struct TcpTunneler {
    tunneled: Stream,
}

impl TcpTunneler {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let to_address = format!("{}:{}", address, port);
        log::debug!("connecting to {}", to_address);
        let stream = TcpStream::connect(to_address).await?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            tunneled: Stream::new(reader, writer),
        })
    }
}

#[async_trait]
impl Tunneler for TcpTunneler {
    async fn tunnel(&mut self, mut client: Stream) -> Result<(), Box<dyn Error>> {
        let to_tunnel_future = copy(
            &mut client.reader,
            &mut self.tunneled.writer,
            "sending to tunnel",
        );
        let from_tunnel_future = copy(
            &mut self.tunneled.reader,
            &mut client.writer,
            "received from tunnel",
        );
        match tokio::try_join!(to_tunnel_future, from_tunnel_future) {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio_test::io::Builder;

    use super::*;

    #[tokio::test]
    async fn tcp_single_read() -> Result<(), Box<dyn Error>> {
        let tunneled_client = Stream::new(
            Builder::new().build(),
            Builder::new().write(b"hello").build(),
        );
        let mut tunneler = TcpTunneler {
            tunneled: tunneled_client,
        };

        let client = Stream::new(
            Builder::new().read(b"hello").build(),
            Builder::new().build(),
        );
        tunneler.tunnel(client).await
    }

    #[tokio::test]
    async fn tcp_single_write() -> Result<(), Box<dyn Error>> {
        let tunneled_client = Stream::new(
            Builder::new().read(b"hello").build(),
            Builder::new().build(),
        );
        let mut tunneler = TcpTunneler {
            tunneled: tunneled_client,
        };

        let client = Stream::new(
            Builder::new().build(),
            Builder::new().write(b"hello").build(),
        );
        tunneler.tunnel(client).await
    }

    #[tokio::test]
    async fn tcp_single_read_write() -> Result<(), Box<dyn Error>> {
        let tunneled_client = Stream::new(
            Builder::new().read(b"hello").build(),
            Builder::new().write(b"world").build(),
        );
        let mut tunneler = TcpTunneler {
            tunneled: tunneled_client,
        };

        let client = Stream::new(
            Builder::new().read(b"world").build(),
            Builder::new().write(b"hello").build(),
        );
        tunneler.tunnel(client).await
    }

    #[tokio::test]
    async fn tcp_multiple_reads() -> Result<(), Box<dyn Error>> {
        let tunneled_client = Stream::new(
            Builder::new().build(),
            Builder::new().write(b"1").write(b"2").write(b"3").build(),
        );
        let mut tunneler = TcpTunneler {
            tunneled: tunneled_client,
        };

        let client = Stream::new(
            Builder::new().read(b"1").read(b"2").read(b"3").build(),
            Builder::new().build(),
        );
        tunneler.tunnel(client).await
    }

    #[tokio::test]
    async fn tcp_multiple_writes() -> Result<(), Box<dyn Error>> {
        let tunneled_client = Stream::new(
            Builder::new().build(),
            Builder::new().write(b"1").write(b"2").write(b"3").build(),
        );
        let mut tunneler = TcpTunneler {
            tunneled: tunneled_client,
        };

        let client = Stream::new(
            Builder::new().read(b"1").read(b"2").read(b"3").build(),
            Builder::new().build(),
        );
        tunneler.tunnel(client).await
    }
}
