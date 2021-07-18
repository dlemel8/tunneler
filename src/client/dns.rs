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

use common::dns::{
    new_client_id, ClientId, ClientIdSuffixEncoder, Decoder, Encoder, HexDecoder, HexEncoder,
};
use common::io::{AsyncReader, AsyncWriter};

use crate::tunnel::Tunneler;

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

pub(crate) struct DnsTunneler {
    encoder: Box<dyn Encoder>,
    client_id: ClientId,
    client: Box<dyn AsyncDnsClient>,
    decoder: Box<dyn Decoder>,
}

impl DnsTunneler {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let socket = SocketAddr::new(address, port);
        let stream = UdpClientStream::<UdpSocket>::new(socket);
        let (client, background) = AsyncClient::connect(stream).await?;
        tokio::spawn(background);
        Ok(DnsTunneler {
            encoder: Box::new(ClientIdSuffixEncoder::new(HexEncoder {})),
            client_id: new_client_id(),
            client: Box::new(AsyncClientWrapper { client }),
            decoder: Box::new(HexDecoder {}),
        })
    }
}

#[async_trait]
impl Tunneler for DnsTunneler {
    async fn tunnel(
        &mut self,
        mut to_tunnel: Box<dyn AsyncReader>,
        mut from_tunnel: Box<dyn AsyncWriter>,
    ) -> Result<(), Box<dyn Error>> {
        let mut data_to_tunnel = vec![0; MAXIMUM_LABEL_SIZE];
        let read_limit = self
            .encoder
            .calculate_max_decoded_size(data_to_tunnel.len());

        loop {
            let size = self
                .read_data_to_tunnel(&mut to_tunnel, &mut data_to_tunnel, read_limit)
                .await?;
            if size == 0 {
                break;
            }

            let encoded_data_to_tunnel = self.encoder.encode(&data_to_tunnel[..size]);
            log::debug!("sending to tunnel {:?}", encoded_data_to_tunnel);
            let encoded_data_from_tunnel = self.send_dns_query(encoded_data_to_tunnel).await?;

            log::debug!("received from tunnel {:?}", encoded_data_from_tunnel);
            let data_from_tunnel = self.decoder.decode(&encoded_data_from_tunnel)?;
            from_tunnel.write(data_from_tunnel.as_slice()).await?;
        }
        Ok(())
    }
}

impl DnsTunneler {
    async fn read_data_to_tunnel(
        &mut self,
        to_tunnel: &mut Box<dyn AsyncReader>,
        data_to_tunnel: &mut Vec<u8>,
        max_decoded_size: usize,
    ) -> Result<usize, Box<dyn Error>> {
        let size = to_tunnel
            .read(&mut data_to_tunnel[..max_decoded_size])
            .await?;
        if size == 0 {
            return Ok(0);
        }

        data_to_tunnel[size..size + self.client_id.len()].copy_from_slice(&self.client_id);
        Ok(size + self.client_id.len())
    }

    async fn send_dns_query(
        &mut self,
        encoded_data_to_tunnel: String,
    ) -> Result<String, Box<dyn Error>> {
        let response = self
            .client
            .query(
                Name::from_str(encoded_data_to_tunnel.as_str())?,
                DNSClass::IN,
                RecordType::TXT,
            )
            .await?;

        let answers = response.answers();
        let answer = match answers.len() {
            1 => &answers[0],
            x => return Err(format!("unexpected answers count {}", x).into()),
        };

        match answer.rdata() {
            RData::TXT(text) => Ok(format!("{}", text)),
            x => Err(format!("unexpected answer record data {}", x.to_record_type()).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use mockall::mock;
    use tokio::io;
    use tokio::io::ErrorKind;
    use tokio_test::io::Builder;
    use trust_dns_client::op::Message;
    use trust_dns_client::proto::rr::rdata::TXT;
    use trust_dns_client::proto::rr::Record;

    use common::io::{AsyncReadWrapper, AsyncWriteWrapper};

    use super::*;

    mock! {
        Encoder{}
        impl Encoder for Encoder {
            fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize;
            fn encode(&self, data: &[u8]) -> String;
        }
    }

    mock! {
        Decoder{}
        impl Decoder for Decoder {
            fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>>;
        }
    }

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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
            client_id: [1, 2, 3, 4],
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
