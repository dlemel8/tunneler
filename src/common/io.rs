use std::io;

use async_channel::Sender;
use async_trait::async_trait;
use std::error::Error;
use std::net::IpAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[async_trait]
pub trait AsyncReader: Send {
    async fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>;
}

pub struct AsyncReadWrapper<R: AsyncReadExt + Unpin> {
    reader: R,
}

impl<R: AsyncReadExt + Unpin> AsyncReadWrapper<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }
}

#[async_trait]
impl<R: AsyncReadExt + Unpin + Send> AsyncReader for AsyncReadWrapper<R> {
    async fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>
    where
        Self: Unpin,
    {
        self.reader.read(buf).await
    }
}

#[async_trait]
pub trait AsyncWriter: Send {
    async fn write<'a>(&'a mut self, src: &'a [u8]) -> io::Result<usize>;
    async fn shutdown(&mut self) -> io::Result<()>;
}

pub struct AsyncWriteWrapper<W: AsyncWriteExt + Unpin> {
    writer: W,
}

impl<W: AsyncWriteExt + Unpin> AsyncWriteWrapper<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

#[async_trait]
impl<W: AsyncWriteExt + Unpin + Send> AsyncWriter for AsyncWriteWrapper<W> {
    async fn write<'a>(&'a mut self, src: &'a [u8]) -> io::Result<usize>
    where
        Self: Unpin,
    {
        self.writer.write(src).await
    }

    async fn shutdown(&mut self) -> io::Result<()> {
        self.writer.shutdown().await
    }
}

pub async fn copy(
    from: &mut Box<dyn AsyncReader>,
    to: &mut Box<dyn AsyncWriter>,
    debug_log_message: &str,
) -> tokio::io::Result<()> {
    let mut data_to_tunnel = vec![0; 4096];
    loop {
        let size = from.read(&mut data_to_tunnel).await?;
        if size == 0 {
            break;
        }
        log::debug!("{} {:?}", debug_log_message, &data_to_tunnel[..size]);
        to.write(&data_to_tunnel[..size]).await?;
    }
    to.shutdown().await
}

pub struct Client {
    pub reader: Box<dyn AsyncReader>,
    pub writer: Box<dyn AsyncWriter>,
}

pub struct TcpServer {
    listener: TcpListener,
}

impl TcpServer {
    pub async fn new(local_address: IpAddr, local_port: u16) -> Result<Self, Box<dyn Error>> {
        let listener_address = format!("{}:{}", local_address, local_port);
        log::info!("start listening on {}", listener_address);
        let listener = TcpListener::bind(listener_address).await?;
        Ok(Self { listener })
    }

    pub async fn accept_clients(
        &mut self,
        new_clients: Sender<Client>,
    ) -> Result<(), Box<dyn Error>> {
        while let Ok((client_stream, client_address)) = self.listener.accept().await {
            log::debug!("got connection from {}", client_address);
            let (client_reader, client_writer) = client_stream.into_split();
            let reader = Box::new(AsyncReadWrapper::new(client_reader));
            let writer = Box::new(AsyncWriteWrapper::new(client_writer));
            new_clients.send(Client { reader, writer }).await?;
        }
        Ok(())
    }
}
