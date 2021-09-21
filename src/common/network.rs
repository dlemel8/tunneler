use std::convert::TryFrom;
use std::error::Error;
use std::net::IpAddr;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::net;
use tokio::time::{timeout as tokio_timeout, Duration};

use crate::io::{AsyncReader, AsyncWriter, Stream};

#[async_trait]
pub trait Listener {
    async fn accept_clients(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>>;
}

pub struct TcpListener {
    listener: net::TcpListener,
}

impl TcpListener {
    pub async fn new(local_address: IpAddr, local_port: u16) -> Result<Self, Box<dyn Error>> {
        let listener_address = format!("{}:{}", local_address, local_port);
        log::info!("start listening on {}", listener_address);
        let listener = net::TcpListener::bind(listener_address).await?;
        Ok(Self { listener })
    }
}

#[async_trait]
impl Listener for TcpListener {
    async fn accept_clients(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        while let Ok((client_stream, client_address)) = self.listener.accept().await {
            log::debug!("got connection from {}", client_address);
            let (client_reader, client_writer) = client_stream.into_split();
            new_clients
                .send(Stream::new(client_reader, client_writer))
                .await?;
        }
        Ok(())
    }
}

pub const MAX_UDP_PACKET_SIZE: usize = u16::MAX as usize;
pub const STREAMED_UDP_PACKET_HEADER_SIZE: usize = 2;

pub async fn stream_udp_packet(payload: &Vec<u8>, size: usize, writer: &mut Box<dyn AsyncWriter>) {
    if payload.len() < size {
        log::error!(
            "payload {:?} is too small (expecting size {})",
            payload,
            size
        );
        return;
    }

    let size_u16 = match u16::try_from(size) {
        Ok(s) => s,
        Err(e) => {
            log::error!("size {} can't fit in a u16: {}", size, e);
            return;
        }
    };

    if let Err(e) = writer.write(&size_u16.to_be_bytes()).await {
        log::error!("failed to write header: {}", e);
        return;
    };

    if let Err(e) = writer.write(&payload[..size]).await {
        log::error!("failed to write payload: {}", e);
        return;
    };
}

#[derive(PartialEq, Debug)]
pub enum UnstreamPacketResult {
    Error,
    Timeout,
    Payload(Vec<u8>),
}

pub async fn unstream_udp_packet(
    reader: &mut Box<dyn AsyncReader>,
    timeout: Option<Duration>,
) -> UnstreamPacketResult {
    let mut header_bytes = [0; STREAMED_UDP_PACKET_HEADER_SIZE];
    let read_header_future = reader.read_exact(&mut header_bytes);
    let header_size_result = match timeout {
        None => read_header_future.await,
        Some(duration) => match tokio_timeout(duration, read_header_future).await {
            Ok(size_result) => size_result,
            Err(_) => return UnstreamPacketResult::Timeout,
        },
    };

    let header_size = match header_size_result {
        Ok(size) => size,
        Err(e) => {
            log::error!("failed to read header: {}", e);
            return UnstreamPacketResult::Error;
        }
    };

    if header_size != STREAMED_UDP_PACKET_HEADER_SIZE {
        log::error!("got unexpected header size in bytes {}", header_size);
        return UnstreamPacketResult::Error;
    }

    let header = u16::from_be_bytes(header_bytes);
    let header_usize = header as usize;
    let mut payload = vec![0; header_usize];
    let size = match reader.read_exact(&mut payload).await {
        Ok(size) => size,
        Err(e) => {
            log::error!("failed to read payload: {}", e);
            return UnstreamPacketResult::Error;
        }
    };

    if size != header_usize {
        log::error!("got unexpected data size in bytes {}", header_size);
        return UnstreamPacketResult::Error;
    }

    UnstreamPacketResult::Payload(payload)
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use tokio::io;
    use tokio_test::io::Builder;

    use crate::io::{AsyncReadWrapper, AsyncWriteWrapper};

    use super::*;

    #[tokio::test]
    async fn stream_udp_packet_payload_too_small() -> Result<(), Box<dyn Error>> {
        let payload = vec![1, 2, 3];
        let mut writer: Box<dyn AsyncWriter> =
            Box::new(AsyncWriteWrapper::new(Builder::new().build()));

        stream_udp_packet(&payload, 7, &mut writer).await;
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_size_not_fit_in_u16() -> Result<(), Box<dyn Error>> {
        let payload = vec![0; u16::MAX as usize + 7];
        let mut writer: Box<dyn AsyncWriter> =
            Box::new(AsyncWriteWrapper::new(Builder::new().build()));

        stream_udp_packet(&payload, payload.len(), &mut writer).await;
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_write_header_failed() -> Result<(), Box<dyn Error>> {
        let payload = vec![1, 2, 3];
        let mut writer: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(
            Builder::new()
                .write_error(io::Error::new(ErrorKind::Other, "oh no!"))
                .build(),
        ));

        stream_udp_packet(&payload, payload.len(), &mut writer).await;
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_write_payload_failed() -> Result<(), Box<dyn Error>> {
        let payload = vec![1, 2, 3];
        let mut writer: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(
            Builder::new()
                .write(vec![0u8, 3].as_slice())
                .write_error(io::Error::new(ErrorKind::Other, "oh no!"))
                .build(),
        ));

        stream_udp_packet(&payload, payload.len(), &mut writer).await;
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_success() -> Result<(), Box<dyn Error>> {
        let payload = vec![1, 2, 3];
        let mut writer: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(
            Builder::new()
                .write(vec![0u8, 3].as_slice())
                .write(payload.as_slice())
                .build(),
        ));

        stream_udp_packet(&payload, payload.len(), &mut writer).await;
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_timeout() -> Result<(), Box<dyn Error>> {
        let mut reader: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(
            Builder::new().wait(Duration::from_secs(5)).build(),
        ));

        let res = unstream_udp_packet(&mut reader, Some(Duration::from_millis(1))).await;
        assert_eq!(res, UnstreamPacketResult::Timeout);
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_read_header_failed() -> Result<(), Box<dyn Error>> {
        let mut reader: Box<dyn AsyncReader> =
            Box::new(AsyncReadWrapper::new(Builder::new().build()));

        let res = unstream_udp_packet(&mut reader, None).await;
        assert_eq!(res, UnstreamPacketResult::Error);
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_read_payload_failed() -> Result<(), Box<dyn Error>> {
        let mut reader: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(
            Builder::new().read(vec![0u8, 3].as_slice()).build(),
        ));

        let res = unstream_udp_packet(&mut reader, None).await;
        assert_eq!(res, UnstreamPacketResult::Error);
        Ok(())
    }

    #[tokio::test]
    async fn stream_udp_packet_read_payload_success() -> Result<(), Box<dyn Error>> {
        let payload = vec![1u8, 2, 3];
        let mut reader: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(
            Builder::new()
                .read(vec![0u8, 3].as_slice())
                .read(payload.as_slice())
                .build(),
        ));

        let res = unstream_udp_packet(&mut reader, None).await;
        assert_eq!(res, UnstreamPacketResult::Payload(payload));
        Ok(())
    }
}
