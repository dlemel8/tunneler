use std::error::Error;
use std::net::IpAddr;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::io;
use tokio::net::TcpStream;

#[async_trait]
pub(crate) trait AsyncReader: Send {
    async fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>;
}

pub(crate) struct AsyncReadWrapper<R: AsyncReadExt + Unpin> {
    reader: R,
}

impl<R: AsyncReadExt + Unpin> AsyncReadWrapper<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self { reader }
    }
}

#[async_trait]
impl<R: AsyncReadExt + Unpin + Send> AsyncReader for AsyncReadWrapper<R> {
    async fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize> where Self: Unpin {
        self.reader.read(buf).await
    }
}

#[async_trait]
pub(crate) trait AsyncWriter: Send {
    async fn write<'a>(&'a mut self, src: &'a [u8]) -> io::Result<usize>;
    async fn shutdown(&mut self) -> io::Result<()>;
}

pub(crate) struct AsyncWriteWrapper<W: AsyncWriteExt + Unpin> {
    writer: W,
}

impl<W: AsyncWriteExt + Unpin> AsyncWriteWrapper<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self { writer }
    }
}

#[async_trait]
impl<W: AsyncWriteExt + Unpin + Send> AsyncWriter for AsyncWriteWrapper<W> {
    async fn write<'a>(&'a mut self, src: &'a [u8]) -> io::Result<usize> where Self: Unpin {
        self.writer.write(src).await
    }

    async fn shutdown(&mut self) -> io::Result<()> {
        self.writer.shutdown().await
    }
}

#[async_trait]
pub(crate) trait Tunneler: Send {
    async fn tunnel(&mut self, reader: Box<dyn AsyncReader>, writer: Box<dyn AsyncWriter>) -> Result<(), Box<dyn Error>>;
}

pub(crate) struct TcpTunneler {
    reader: Box<dyn AsyncReader>,
    writer: Box<dyn AsyncWriter>,
}

impl TcpTunneler {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let to_address = format!("{}:{}", address, port);
        log::debug!("connecting to {}", to_address);
        let stream = TcpStream::connect(to_address).await?;
        let (out_read, out_write) = stream.into_split();
        let reader = Box::new(AsyncReadWrapper::new(out_read));
        let writer = Box::new(AsyncWriteWrapper::new(out_write));
        Ok(Self { reader, writer })
    }
}

#[async_trait]
impl Tunneler for TcpTunneler {
    async fn tunnel(&mut self, mut reader: Box<dyn AsyncReader>, mut writer: Box<dyn AsyncWriter>) -> Result<(), Box<dyn Error>> {
        let self_writer = &mut self.writer;
        let in_to_out = async {
            let mut data_to_send = vec![0; 4096];
            loop {
                let size = reader.read(&mut data_to_send).await?;
                if size == 0 {
                    break;
                }
                log::debug!("going to send {:?}", &data_to_send[..size]);
                self_writer.write(&data_to_send[..size]).await?;
            }
            self_writer.shutdown().await
        };

        let self_reader = &mut self.reader;
        let out_to_in = async {
            let mut data_to_send = vec![0; 4096];
            loop {
                let size = self_reader.read(&mut data_to_send).await?;
                if size == 0 {
                    break;
                }
                log::debug!("going to send {:?}", &data_to_send[..size]);
                writer.write(&data_to_send[..size]).await?;
            }
            writer.shutdown().await
        };

        match tokio::try_join!(in_to_out, out_to_in) {
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
    async fn single_read() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().write(b"hello").build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler{reader: tunneler_reader, writer: tunneler_writer};

        let tunneled_read_mock = Builder::new().read(b"hello").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn single_write() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().read(b"hello").build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler{reader: tunneler_reader, writer: tunneler_writer};

        let tunneled_read_mock = Builder::new().build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"hello").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn single_read_write() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().read(b"hello").build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().write(b"world").build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler{reader: tunneler_reader, writer: tunneler_writer};

        let tunneled_read_mock = Builder::new().read(b"world").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"hello").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn multiple_reads() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().write(b"1").write(b"2").write(b"3").build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler{reader: tunneler_reader, writer: tunneler_writer};

        let tunneled_read_mock = Builder::new().read(b"1").read(b"2").read(b"3").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn multiple_writes() -> Result<(), Box<dyn Error>> {
        let tunneler_read_mock = Builder::new().read(b"1").read(b"2").read(b"3").build();
        let tunneler_reader = Box::new(AsyncReadWrapper::new(tunneler_read_mock));
        let tunneler_write_mock = Builder::new().build();
        let tunneler_writer = Box::new(AsyncWriteWrapper::new(tunneler_write_mock));
        let mut tunneler = TcpTunneler{reader: tunneler_reader, writer: tunneler_writer};

        let tunneled_read_mock = Builder::new().build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"1").write(b"2").write(b"3").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }
}