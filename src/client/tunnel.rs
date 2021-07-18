use std::error::Error;
use std::net::IpAddr;

use async_trait::async_trait;
use tokio::net::TcpStream;

use common::io::{copy, AsyncReadWrapper, AsyncReader, AsyncWriteWrapper, AsyncWriter};

#[async_trait(?Send)]
pub(crate) trait Tunneler {
    async fn tunnel(
        &mut self,
        to_tunnel: Box<dyn AsyncReader>,
        from_tunnel: Box<dyn AsyncWriter>,
    ) -> Result<(), Box<dyn Error>>;
}

pub(crate) struct TcpTunneler {
    from_tunnel: Box<dyn AsyncReader>,
    to_tunnel: Box<dyn AsyncWriter>,
}

impl TcpTunneler {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let to_address = format!("{}:{}", address, port);
        log::debug!("connecting to {}", to_address);
        let stream = TcpStream::connect(to_address).await?;
        let (out_read, out_write) = stream.into_split();
        let reader = Box::new(AsyncReadWrapper::new(out_read));
        let writer = Box::new(AsyncWriteWrapper::new(out_write));
        Ok(Self {
            from_tunnel: reader,
            to_tunnel: writer,
        })
    }
}

#[async_trait(?Send)]
impl Tunneler for TcpTunneler {
    async fn tunnel(
        &mut self,
        mut to_tunnel: Box<dyn AsyncReader>,
        mut from_tunnel: Box<dyn AsyncWriter>,
    ) -> Result<(), Box<dyn Error>> {
        let to_tunnel_future = copy(&mut to_tunnel, &mut self.to_tunnel, "sending to tunnel");
        let from_tunnel_future = copy(
            &mut self.from_tunnel,
            &mut from_tunnel,
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
        let tunneler_read_mock = Builder::new().build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().write(b"hello").build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler {
            from_tunnel: tunneler_reader,
            to_tunnel: tunneler_writer,
        };

        let tunneled_read_mock = Builder::new().read(b"hello").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn tcp_single_write() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().read(b"hello").build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler {
            from_tunnel: tunneler_reader,
            to_tunnel: tunneler_writer,
        };

        let tunneled_read_mock = Builder::new().build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"hello").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn tcp_single_read_write() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().read(b"hello").build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().write(b"world").build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler {
            from_tunnel: tunneler_reader,
            to_tunnel: tunneler_writer,
        };

        let tunneled_read_mock = Builder::new().read(b"world").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"hello").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn tcp_multiple_reads() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().write(b"1").write(b"2").write(b"3").build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler {
            from_tunnel: tunneler_reader,
            to_tunnel: tunneler_writer,
        };

        let tunneled_read_mock = Builder::new().read(b"1").read(b"2").read(b"3").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn tcp_multiple_writes() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().read(b"1").read(b"2").read(b"3").build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler {
            from_tunnel: tunneler_reader,
            to_tunnel: tunneler_writer,
        };

        let tunneled_read_mock = Builder::new().build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"1").write(b"2").write(b"3").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }
}
