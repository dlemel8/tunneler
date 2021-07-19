use std::array::IntoIter;
use std::convert::TryInto;
use std::error::Error;
use std::future::Future;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use tokio::io::{duplex, split};
use tokio::net::UdpSocket;
use trust_dns_client::op::{Header, MessageType, OpCode};
use trust_dns_client::proto::rr::rdata::TXT;
use trust_dns_client::proto::rr::{DNSClass, RData, Record, RecordType};
use trust_dns_server::authority::{MessageRequest, MessageResponse, MessageResponseBuilder};
use trust_dns_server::server::{Request, RequestHandler, ResponseHandler};
use trust_dns_server::ServerFuture;

use common::dns::{
    ClientId, ClientIdSuffixDecoder, Decoder, Encoder, HexDecoder, HexEncoder,
    CLIENT_ID_SIZE_IN_BYTES,
};
use common::io::Stream;

use crate::io::{StreamCreator, StreamsCache};
use crate::tunnel::Untunneler;
use tokio::time::{Duration, Instant};

pub(crate) struct DnsUntunneler {
    listener_address: SocketAddr,
}

impl DnsUntunneler {
    pub(crate) async fn new(
        local_address: IpAddr,
        local_port: u16,
    ) -> Result<Self, Box<dyn Error>> {
        let listener_address = SocketAddr::new(local_address, local_port);
        Ok(Self { listener_address })
    }
}

#[async_trait(? Send)]
impl Untunneler for DnsUntunneler {
    async fn untunnel(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        let cache = StreamsCache::new(
            move || {
                let (local, remote) = duplex(4096);

                let (remote_reader, remote_writer) = split(remote);
                new_clients.try_send(Stream::new(remote_reader, remote_writer))?;

                let (local_reader, local_writer) = split(local);
                Ok(Stream::new(local_reader, local_writer))
            },
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );

        let udp_socket = UdpSocket::bind(self.listener_address).await?;
        let mut server = ServerFuture::new(UntunnelRequestHandler::new(cache));
        server.register_socket(udp_socket);
        match server.block_until_done().await {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg_attr(test, automock)]
pub(crate) trait DnsResponseHandler {
    #[allow(clippy::needless_lifetimes)]
    fn send_response<'a, 'b>(&mut self, response: MessageResponse<'a, 'b>) -> io::Result<()>;
}

struct ResponseHandlerWrapper<R: ResponseHandler> {
    handler: R,
}

impl<R: ResponseHandler> DnsResponseHandler for ResponseHandlerWrapper<R> {
    fn send_response(&mut self, response: MessageResponse) -> io::Result<()> {
        self.handler.send_response(response)
    }
}

const MAXIMUM_TXT_RECORD_SIZE: usize = 54;

pub struct UntunnelRequestHandler<F: StreamCreator> {
    untunneled_clients: Arc<StreamsCache<F, ClientId>>,
}

impl<F: StreamCreator> UntunnelRequestHandler<F> {
    pub(crate) fn new(cache: StreamsCache<F, ClientId>) -> Self {
        Self {
            untunneled_clients: Arc::new(cache),
        }
    }
}

impl<F: StreamCreator> RequestHandler for UntunnelRequestHandler<F> {
    type ResponseFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

    fn handle_request<R: ResponseHandler>(
        &self,
        request: Request,
        response_handle: R,
    ) -> Self::ResponseFuture {
        Box::pin(untunnel_request(
            request,
            ResponseHandlerWrapper {
                handler: response_handle,
            },
            self.untunneled_clients.clone(),
            ClientIdSuffixDecoder::new(HexDecoder {}),
            HexEncoder {},
        ))
    }
}

async fn untunnel_request<R: DnsResponseHandler, F: StreamCreator, D: Decoder, E: Encoder>(
    request: Request,
    mut response_handle: R,
    untunneled_clients: Arc<StreamsCache<F, ClientId>>,
    decoder: D,
    encoder: E,
) {
    let message = &request.message;
    let encoded_data_from_tunnel = match get_data_from_tunnel(message) {
        Some(x) => x,
        None => return,
    };

    log::debug!("received from tunnel {:?}", encoded_data_from_tunnel);
    let data_from_tunnel = match decoder.decode(&encoded_data_from_tunnel) {
        Ok(x) => x,
        Err(e) => {
            log::error!("{}: failed to decode: {}", message.id(), e);
            return;
        }
    };

    let data_to_tunnel = match untunnel_data(
        untunneled_clients,
        data_from_tunnel,
        message.id(),
        encoder.calculate_max_decoded_size(MAXIMUM_TXT_RECORD_SIZE),
    )
    .await
    {
        Some(x) => x,
        None => return,
    };

    let encoded_data_to_tunnel = match encoder.encode(&data_to_tunnel) {
        Ok(x) => x,
        Err(e) => {
            log::error!("{}: failed to encode: {}", message.id(), e);
            return;
        }
    };
    log::debug!("sending to tunnel {:?}", encoded_data_to_tunnel);
    let answer = Record::new()
        .set_rdata(RData::TXT(TXT::new(vec![encoded_data_to_tunnel])))
        .set_record_type(RecordType::TXT)
        .set_dns_class(DNSClass::IN)
        .clone();

    let response = create_response(message, &answer);
    response_handle.send_response(response).unwrap_or_else(|e| {
        log::error!("{}: failed to send response: {}", message.id(), e);
    });
}

fn get_data_from_tunnel(message: &MessageRequest) -> Option<String> {
    let first_query = match message.queries().len() {
        1 => (&message.queries()[0]).original(),
        x => {
            log::error!("{}: unexpected number of queries {}", message.id(), x);
            return None;
        }
    };

    let data_from_tunnel = match first_query.query_type() {
        RecordType::TXT => first_query
            .name()
            .to_string()
            .trim_end_matches('.')
            .to_string(),
        x => {
            log::error!("{}: unexpected type of query {}", message.id(), x);
            return None;
        }
    };

    Some(data_from_tunnel)
}

async fn untunnel_data<F: StreamCreator>(
    clients: Arc<StreamsCache<F, ClientId>>,
    data: Vec<u8>,
    message_id: u16,
    data_to_tunnel_max_size: usize,
) -> Option<Vec<u8>> {
    if data.len() < CLIENT_ID_SIZE_IN_BYTES + 1 {
        log::error!(
            "{}: decode return value too small: {}",
            message_id,
            data.len()
        );
        return None;
    }

    let (data_to_write, client_id) = data.split_at(data.len() - CLIENT_ID_SIZE_IN_BYTES);
    let client = match clients.get(client_id.try_into().unwrap(), Instant::now()) {
        Ok(x) => x,
        Err(e) => {
            log::error!("{}: failed to get client: {}", message_id, e);
            return None;
        }
    };

    if let Err(e) = client.lock().await.writer.write(data_to_write).await {
        log::error!("failed to write to client: {}", e);
        return None;
    };

    let mut data_to_tunnel = vec![0; data_to_tunnel_max_size];
    let size = match client.lock().await.reader.read(&mut data_to_tunnel).await {
        Ok(x) => x,
        Err(e) => {
            log::error!("failed to read from client: {}", e);
            return None;
        }
    };

    data_to_tunnel.truncate(size);
    Some(data_to_tunnel)
}

fn create_response<'a>(message: &'a MessageRequest, answer: &'a Record) -> MessageResponse<'a, 'a> {
    let builder = MessageResponseBuilder::new(Option::from(message.raw_queries()));
    let mut response_header = Header::new();
    response_header.set_id(message.id());
    response_header.set_op_code(OpCode::Query);
    response_header.set_message_type(MessageType::Response);
    response_header.set_authoritative(true);

    builder.build(
        response_header,
        new_iterator(Some(answer)),
        new_iterator(None),
        new_iterator(None),
        new_iterator(None),
    )
}

fn new_iterator<'a>(
    record: Option<&'a Record>,
) -> Box<dyn Iterator<Item = &'a Record> + Send + 'a> {
    match record {
        None => Box::new(IntoIter::new([])),
        Some(r) => Box::new(IntoIter::new([r])),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use mockall::mock;
    use tokio_test::io::Builder;
    use trust_dns_client::op::Message;
    use trust_dns_client::proto::serialize::binary::BinDecodable;
    use trust_dns_server::authority::MessageRequest;
    use trust_dns_server::proto::op::Query;

    use super::*;
    use tokio::io::ErrorKind;

    mock! {
        Encoder{}
        impl Encoder for Encoder {
            fn calculate_max_decoded_size(&self, max_encoded_size: usize) -> usize;
            fn encode(&self, data: &[u8]) -> Result<String, Box<dyn Error>>;
        }
    }

    mock! {
        Decoder{}
        impl Decoder for Decoder {
            fn decode(&self, data: &str) -> Result<Vec<u8>, Box<dyn Error>>;
        }
    }

    #[tokio::test]
    async fn dns_empty_message() -> Result<(), Box<dyn Error>> {
        let message = Message::default();
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let decoder_mock = MockDecoder::new();
        let encoder_mock = MockEncoder::new();

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_multiple_queries_message() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let queries = message.queries_mut();
        queries.push(Query::default());
        queries.push(Query::default());
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let decoder_mock = MockDecoder::new();
        let encoder_mock = MockEncoder::new();

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_non_txt_query_message() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        message.queries_mut().push(Query::default());
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let decoder_mock = MockDecoder::new();
        let encoder_mock = MockEncoder::new();

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_failed_to_decode() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Err(String::from("bla").into()));
        let encoder_mock = MockEncoder::new();

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_decode_return_value_too_small() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || Ok(Stream::new(Builder::new().build(), Builder::new().build())),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("12").into_bytes()));
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_failed_to_get_client() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || Err(String::from("bla").into()),
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("bla1234").into_bytes()));
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_failed_to_write_to_client() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || {
                Ok(Stream::new(
                    Builder::new().build(),
                    Builder::new()
                        .write_error(io::Error::new(ErrorKind::Other, "oh no!"))
                        .build(),
                ))
            },
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("bla1234").into_bytes()));
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_failed_to_read_from_client() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || {
                Ok(Stream::new(
                    Builder::new()
                        .read_error(io::Error::new(ErrorKind::Other, "oh no!"))
                        .build(),
                    Builder::new().write(b"bla").build(),
                ))
            },
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("bla1234").into_bytes()));
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_failed_to_encode() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let handler_mock = MockDnsResponseHandler::new();
        let cache = StreamsCache::new(
            || {
                Ok(Stream::new(
                    Builder::new().read(b"bli").build(),
                    Builder::new().write(b"bla").build(),
                ))
            },
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("bla1234").into_bytes()));
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock
            .expect_encode()
            .returning(|_| Err(String::from("bla").into()));

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_failed_to_send_response() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let mut handler_mock = MockDnsResponseHandler::new();
        handler_mock
            .expect_send_response()
            .returning(|_| Err(io::Error::new(ErrorKind::Other, "oh no!")));
        let cache = StreamsCache::new(
            || {
                Ok(Stream::new(
                    Builder::new().read(b"bli").build(),
                    Builder::new().write(b"bla").build(),
                ))
            },
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("bla1234").into_bytes()));
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock
            .expect_encode()
            .returning(|_| Ok(String::from("encoded")));

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }

    #[tokio::test]
    async fn dns_success() -> Result<(), Box<dyn Error>> {
        let mut message = Message::default();
        let mut query = Query::default();
        query.set_query_type(RecordType::TXT);
        message.queries_mut().push(query);
        let message_bytes = message.to_vec().unwrap();
        let request = MessageRequest::from_bytes(&message_bytes).unwrap();
        let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let request = Request {
            message: request,
            src: socket,
        };

        let mut handler_mock = MockDnsResponseHandler::new();
        handler_mock.expect_send_response().returning(|_| Ok(()));
        let cache = StreamsCache::new(
            || {
                Ok(Stream::new(
                    Builder::new().read(b"bli").build(),
                    Builder::new().write(b"bla").build(),
                ))
            },
            Duration::from_secs(3 * 60),
            Duration::from_secs(60),
        );
        let mut decoder_mock = MockDecoder::new();
        decoder_mock
            .expect_decode()
            .returning(|_| Ok(String::from("bla1234").into_bytes()));
        let mut encoder_mock = MockEncoder::new();
        encoder_mock
            .expect_calculate_max_decoded_size()
            .return_const(17 as usize);
        encoder_mock
            .expect_encode()
            .returning(|_| Ok(String::from("encoded")));

        untunnel_request(
            request,
            handler_mock,
            Arc::new(cache),
            decoder_mock,
            encoder_mock,
        )
        .await;
        Ok(())
    }
}
