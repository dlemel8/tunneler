use std::io;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[async_trait]
pub trait AsyncReader: Send {
    async fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>;
    async fn read_exact<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>;
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
    async fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>
    where
        Self: Unpin,
    {
        self.reader.read(buf).await
    }

    async fn read_exact<'a>(&'a mut self, buf: &'a mut [u8]) -> io::Result<usize>
    where
        Self: Unpin,
    {
        self.reader.read_exact(buf).await
    }
}

#[async_trait]
pub trait AsyncWriter: Send {
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
    let mut data = vec![0; 4096];
    loop {
        let size = from.read(&mut data).await?;
        if size == 0 {
            break;
        }
        log::debug!("{} {:?}", debug_log_message, &data[..size]);
        to.write(&data[..size]).await?;
    }
    to.shutdown().await
}

pub struct Stream {
    pub reader: Box<dyn AsyncReader>,
    pub writer: Box<dyn AsyncWriter>,
}

impl Stream {
    pub fn new<
        R: AsyncReadExt + Unpin + Send + 'static,
        W: AsyncWriteExt + Unpin + Send + 'static,
    >(
        reader: R,
        writer: W,
    ) -> Self {
        Self {
            reader: Box::new(AsyncReadWrapper::new(reader)),
            writer: Box::new(AsyncWriteWrapper::new(writer)),
        }
    }
}
