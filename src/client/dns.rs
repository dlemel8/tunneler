use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use tokio::net::UdpSocket;
use trust_dns_client::client::{AsyncClient, ClientHandle};
use trust_dns_client::op::DnsResponse;
use trust_dns_client::proto::rr::Name;
use trust_dns_client::rr::{DNSClass, RData, RecordType};
use trust_dns_client::udp::UdpClientStream;

use crate::tunnel::{AsyncReader, AsyncWriter, Tunneler};

#[cfg_attr(test, automock)]
#[async_trait]
pub(crate) trait AsyncDnsClient: Send {
    async fn query(
        &mut self,
        name: Name,
        query_class: DNSClass,
        query_type: RecordType,
    ) -> Result<DnsResponse, Box<dyn Error>>;
}

struct AsyncClientWrapper {
    client: AsyncClient,
}

#[async_trait]
impl AsyncDnsClient for AsyncClientWrapper {
    async fn query(
        &mut self,
        name: Name,
        query_class: DNSClass,
        query_type: RecordType,
    ) -> Result<DnsResponse, Box<dyn Error>> {
        match self.client.query(name, query_class, query_type).await {
            Ok(r) => Ok(r),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg_attr(test, automock)]
trait Encoder: Send {
    fn encode(&self, data: &[u8]) -> String;
}

struct HexEncoder {}

impl Encoder for HexEncoder {
    fn encode(&self, data: &[u8]) -> String {
        hex::encode(data)
    }
}

pub(crate) struct DnsTunneler {
    encoder: Box<dyn Encoder>,
    client: Box<dyn AsyncDnsClient>,
}

impl DnsTunneler {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let socket = SocketAddr::new(address, port);
        let stream = UdpClientStream::<UdpSocket>::new(socket);
        let (client, background) = AsyncClient::connect(stream).await?;
        tokio::spawn(background);
        Ok(DnsTunneler {
            encoder: Box::new(HexEncoder {}),
            client: Box::new(AsyncClientWrapper { client }),
        })
    }
}

#[async_trait]
impl Tunneler for DnsTunneler {
    async fn tunnel(
        &mut self,
        mut reader: Box<dyn AsyncReader>,
        mut writer: Box<dyn AsyncWriter>,
    ) -> Result<(), Box<dyn Error>> {
        let mut data_to_send = vec![0; 4096]; // TODO - get max from encoder
        let size = reader.read(&mut data_to_send).await?;
        if size == 0 {
            return Ok(());
        }

        let encoded_data_to_send = self.encoder.encode(&data_to_send[..size]);
        log::debug!("going to send {:?}", encoded_data_to_send);
        let response = self
            .client
            .query(
                Name::from_str(encoded_data_to_send.as_str())?,
                DNSClass::IN,
                RecordType::TXT,
            )
            .await?;


        let answers = response.answers();
        let answer = match answers.len() {
            0 => return Ok(()),
            1 => &answers[0],
            x => return Err(format!("unexpected answers count {}", x).into()),
        };

        let encoded_received_data = match answer.rdata() {
            RData::TXT(text) => format!("{}", text),
            x => return Err(format!("unexpected answer record data {}", x.to_record_type()).into()),
        };
        log::debug!("received {:?}", encoded_received_data);
        let data_to_write = hex::decode(encoded_received_data)?;
        match writer.write(data_to_write.as_slice()).await {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio_test::io::Builder;

    use super::*;
    use crate::tunnel::{AsyncReadWrapper, AsyncWriteWrapper};
    use std::fmt;
    use tokio::io;
    use tokio::io::ErrorKind;
    use trust_dns_client::op::Message;
    use trust_dns_client::proto::rr::Record;

    #[derive(Debug)]
    struct TestError {}

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "bla")
        }
    }

    impl Error for TestError {}

    #[tokio::test]
    async fn dns_failed_to_read() -> Result<(), Box<dyn Error>> {
        let encoder_mock = MockEncoder::new();
        let client_mock = MockAsyncDnsClient::new();
        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
        };

        let tunneled_read_mock = Builder::new()
            .read_error(io::Error::new(ErrorKind::Other, "oh no!"))
            .build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn dns_read_end_of_file() -> Result<(), Box<dyn Error>> {
        let encoder_mock = MockEncoder::new();
        let client_mock = MockAsyncDnsClient::new();
        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
        };

        let tunneled_read_mock = Builder::new().build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn dns_query_failed() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock
            .expect_query()
            .returning(|_, _, _| Err(Box::new(TestError {})));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
        };

        let data: &[u8] = b"bla";
        let tunneled_read_mock = Builder::new().read(data).build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn dns_query_returns_empty_response() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock
            .expect_query()
            .returning(|_, _, _| Ok(DnsResponse::from(Message::new())));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
        };

        let data: &[u8] = b"bla";
        let tunneled_read_mock = Builder::new().read(data).build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn dns_query_returns_response_with_multiple_answers() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock
            .expect_query()
            .returning(|_, _, _| {
                let mut message = Message::new();
                message.add_answer(Record::new()).add_answer(Record::new());
                Ok(DnsResponse::from(message))}
            );

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
        };

        let data: &[u8] = b"bla";
        let tunneled_read_mock = Builder::new().read(data).build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn dns_query_returns_response_single_answer_with_wrong_type() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock
            .expect_query()
            .returning(|_, _, _| {
                let mut message = Message::new();
                message.add_answer(Record::new());
                Ok(DnsResponse::from(message))}
            );

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
        };

        let data: &[u8] = b"bla";
        let tunneled_read_mock = Builder::new().read(data).build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }
}
