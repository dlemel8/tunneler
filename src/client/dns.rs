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

const MAXIMUM_LABEL_SIZE: usize = 63;

#[cfg_attr(test, automock)]
trait Encoder: Send {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize;
    fn encode(&self, data: &[u8]) -> String;
}

#[cfg_attr(test, automock)]
trait Decoder: Send {
    fn decode(&self, data: String) -> Result<Vec<u8>, Box<dyn Error>>;
}

#[derive(Clone, Copy)]
struct HexEncoder {}

impl Encoder for HexEncoder {
    fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize {
        max_encoded_size / 2
    }
    fn encode(&self, data: &[u8]) -> String {
        hex::encode(data)
    }
}

impl Decoder for HexEncoder {
    fn decode(&self, data: String) -> Result<Vec<u8>, Box<dyn Error>> {
        match hex::decode(data) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.into()),
        }
    }
}

pub(crate) struct DnsTunneler {
    encoder: Box<dyn Encoder>,
    client: Box<dyn AsyncDnsClient>,
    decoder: Box<dyn Decoder>,
}

impl DnsTunneler {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let socket = SocketAddr::new(address, port);
        let stream = UdpClientStream::<UdpSocket>::new(socket);
        let (client, background) = AsyncClient::connect(stream).await?;
        tokio::spawn(background);
        let encoder = HexEncoder {};
        Ok(DnsTunneler {
            encoder: Box::new(encoder),
            client: Box::new(AsyncClientWrapper { client }),
            decoder: Box::new(encoder),
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
        let mut data_to_send = vec![0; self.encoder.calculate_max_decoded_size(MAXIMUM_LABEL_SIZE)];
        loop {
            let size = reader.read(&mut data_to_send).await?;
            if size == 0 {
                break;
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
                x => {
                    return Err(
                        format!("unexpected answer record data {}", x.to_record_type()).into(),
                    );
                }
            };

            log::debug!("received {:?}", encoded_received_data);
            let data_to_write = self.decoder.decode(encoded_received_data)?;
            writer.write(data_to_write.as_slice()).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use tokio::io;
    use tokio::io::ErrorKind;
    use tokio_test::io::Builder;
    use trust_dns_client::op::Message;
    use trust_dns_client::proto::rr::rdata::TXT;
    use trust_dns_client::proto::rr::Record;

    use crate::tunnel::{AsyncReadWrapper, AsyncWriteWrapper};

    use super::*;

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
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(MockAsyncDnsClient::new()),
            decoder: Box::new(MockDecoder::new()),
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
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(MockAsyncDnsClient::new()),
            decoder: Box::new(MockDecoder::new()),
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
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock
            .expect_query()
            .returning(|_, _, _| Err(Box::new(TestError {})));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(MockDecoder::new()),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").build();
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
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock
            .expect_query()
            .returning(|_, _, _| Ok(DnsResponse::from(Message::new())));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(MockDecoder::new()),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn dns_query_returns_response_with_multiple_answers() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock.expect_query().returning(|_, _, _| {
            let mut message = Message::new();
            message.add_answer(Record::new()).add_answer(Record::new());
            Ok(DnsResponse::from(message))
        });

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(MockDecoder::new()),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn dns_query_returns_response_single_answer_with_wrong_type() -> Result<(), Box<dyn Error>>
    {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock.expect_query().returning(|_, _, _| {
            let mut message = Message::new();
            message.add_answer(Record::new());
            Ok(DnsResponse::from(message))
        });

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(MockDecoder::new()),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn failed_to_decode() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock.expect_query().returning(|_, _, _| {
            let mut record = Record::new();
            record.set_rdata(RData::TXT(TXT::new(vec!["rdata".to_string()])));
            let mut message = Message::new();
            message.add_answer(record);
            Ok(DnsResponse::from(message))
        });

        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Err(Box::new(TestError {})));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(decoder_mock),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn failed_to_write() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock.expect_query().returning(|_, _, _| {
            let mut record = Record::new();
            record.set_rdata(RData::TXT(TXT::new(vec!["rdata".to_string()])));
            let mut message = Message::new();
            message.add_answer(record);
            Ok(DnsResponse::from(message))
        });

        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("decoded").into_bytes()));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(decoder_mock),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new()
            .write_error(io::Error::new(ErrorKind::Other, "oh no!"))
            .build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        let result = tunneler.tunnel(tunneled_reader, tunneled_writer).await;
        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn single_read_write() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock.expect_query().returning(|_, _, _| {
            let mut record = Record::new();
            record.set_rdata(RData::TXT(TXT::new(vec!["rdata".to_string()])));
            let mut message = Message::new();
            message.add_answer(record);
            Ok(DnsResponse::from(message))
        });

        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("decoded").into_bytes()));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(decoder_mock),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"decoded").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }

    #[tokio::test]
    async fn multiple_read_write() -> Result<(), Box<dyn Error>> {
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock.expect_encode().return_const("encoded");

        let mut client_mock = MockAsyncDnsClient::new();
        client_mock.expect_query().returning(|_, _, _| {
            let mut record = Record::new();
            record.set_rdata(RData::TXT(TXT::new(vec!["rdata".to_string()])));
            let mut message = Message::new();
            message.add_answer(record);
            Ok(DnsResponse::from(message))
        });

        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("decoded").into_bytes()));

        let mut tunneler = DnsTunneler {
            encoder: Box::new(encoder_mock),
            client: Box::new(client_mock),
            decoder: Box::new(decoder_mock),
        };

        let tunneled_read_mock = Builder::new().read(b"bla").read(b"bla").build();
        let tunneled_reader = Box::new(AsyncReadWrapper::new(tunneled_read_mock));
        let tunneled_write_mock = Builder::new().write(b"decoded").write(b"decoded").build();
        let tunneled_writer = Box::new(AsyncWriteWrapper::new(tunneled_write_mock));

        tunneler.tunnel(tunneled_reader, tunneled_writer).await
    }
}
