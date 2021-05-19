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
    pub(crate) reader: R,
}

#[async_trait]
impl<R: AsyncReadExt + Unpin + Send> AsyncReader for AsyncReadWrapper<R> {
    async fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize> where Self: Unpin {
        self.reader.read(buf).await
    }
}

#[async_trait]
pub(crate) trait AsyncWriter : Send {
    async fn write<'a>(&'a mut self, src: &'a [u8]) -> io::Result<usize>;
}

pub(crate) struct AsyncWriteWrapper<W: AsyncWriteExt + Unpin> {
    pub(crate) writer: W,
}

#[async_trait]
impl<W: AsyncWriteExt + Unpin + Send> AsyncWriter for AsyncWriteWrapper<W> {
    async fn write<'a>(&'a mut self, src: &'a [u8]) -> io::Result<usize> where Self: Unpin {
        self.writer.write(src).await
    }
}

#[async_trait]
pub(crate) trait Tunnel: Send {
    async fn tunnel(&mut self, reader: Box<dyn AsyncReader>, writer: Box<dyn AsyncWriter>) -> Result<(), Box<dyn Error>>;
}

pub(crate) struct TcpTunnel {
    stream: TcpStream,
}

impl TcpTunnel {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let to_address = format!("{}:{}", address, port);
        log::debug!("connecting to {}", to_address);
        let stream = TcpStream::connect(to_address).await?;
        Ok(TcpTunnel { stream })
    }
}


#[async_trait]
impl Tunnel for TcpTunnel {
    async fn tunnel(&mut self, mut reader: Box<dyn AsyncReader>, mut writer: Box<dyn AsyncWriter>) -> Result<(), Box<dyn Error>> {
        let (mut out_read, mut out_write) = self.stream.split();

        let in_to_out = async {
            let mut data_to_send = vec![0; 4096]; // TODO - get max from encoder
            reader.read(&mut data_to_send).await?;
            out_write.write(data_to_send.as_slice()).await?;
            out_write.shutdown().await
        };
        let out_to_in = async {
            let mut data_to_send = vec![0; 4096]; // TODO - get max from encoder
            out_read.read(&mut data_to_send).await?;
            writer.write(data_to_send.as_slice()).await
        };

        match tokio::try_join!(in_to_out, out_to_in) {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}