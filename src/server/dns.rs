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
use trust_dns_server::authority::{MessageResponse, MessageResponseBuilder};
use trust_dns_server::server::{Request, RequestHandler, ResponseHandler};
use trust_dns_server::ServerFuture;

use common::dns::{
    ClientId, ClientIdSuffixDecoder, Decoder, Encoder, HexDecoder, HexEncoder,
    CLIENT_ID_SIZE_IN_BYTES,
};
use common::io::{AsyncReadWrapper, AsyncReader, AsyncWriteWrapper, AsyncWriter, Stream};

use crate::io::{StreamCreator, StreamsCache};
use crate::tunnel::Untunneler;

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
        let cache = StreamsCache::new(move || {
            let (local, remote) = duplex(4096);

            let (remote_reader, remote_writer) = split(remote);
            let from_remote: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(remote_reader));
            let to_remote: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(remote_writer));
            let remote_stream = Stream {
                reader: from_remote,
                writer: to_remote,
            };
            new_clients.try_send(remote_stream)?;

            let (local_reader, local_writer) = split(local);
            let from_local: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(local_reader));
            let to_local: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(local_writer));
            Ok(Stream {
                reader: from_local,
                writer: to_local,
            })
        });

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
    let message = request.message;
    let builder = MessageResponseBuilder::new(Option::from(message.raw_queries()));

    let mut response_header = Header::new();
    response_header.set_id(message.id());
    response_header.set_op_code(OpCode::Query);
    response_header.set_message_type(MessageType::Response);
    response_header.set_authoritative(true);

    let first_query = match message.queries().len() {
        1 => (&message.queries()[0]).original(),
        x => {
            log::error!("{}: unexpected number of queries {}", message.id(), x);
            return;
        }
    };

    if first_query.query_type() != RecordType::TXT {
        response_handle
            .send_response(builder.build_no_records(response_header))
            .unwrap();
        return;
    }

    let encoded_received_data = first_query
        .name()
        .to_string()
        .trim_end_matches('.')
        .to_string();

    log::debug!("received from tunnel {:?}", encoded_received_data);
    let received_data = decoder.decode(&encoded_received_data).unwrap();
    let (received_data, client_id) =
        received_data.split_at(received_data.len() - CLIENT_ID_SIZE_IN_BYTES);
    let client = untunneled_clients
        .get(client_id.try_into().unwrap())
        .unwrap();
    if let Err(e) = client.lock().await.writer.write(received_data).await {
        log::error!("failed to write to client: {}", e);
        // TODO - send error
        return;
    };

    let mut data_to_tunnel = vec![0; 4096];
    let size = match client.lock().await.reader.read(&mut data_to_tunnel).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("failed to read from client: {}", e);
            // TODO - send error
            return;
        }
    };

    let encoded_data_to_tunnel = encoder.encode(&data_to_tunnel[..size]);
    log::debug!("sending to tunnel {:?}", encoded_data_to_tunnel);
    let answer = Record::new()
        .set_rdata(RData::TXT(TXT::new(vec![encoded_data_to_tunnel])))
        .set_record_type(RecordType::TXT)
        .set_dns_class(DNSClass::IN)
        .clone();

    let response = builder.build(
        response_header,
        new_iterator(Some(&answer)),
        new_iterator(None),
        new_iterator(None),
        new_iterator(None),
    );
    response_handle.send_response(response).unwrap();
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
        let cache = StreamsCache::new(|| {
            let untunneled_read_mock = Builder::new().build();
            let untunneled_reader = Box::new(AsyncReadWrapper::new(untunneled_read_mock));
            let untunneled_write_mock = Builder::new().build();
            let untunneled_writer = Box::new(AsyncWriteWrapper::new(untunneled_write_mock));
            Ok(Stream {
                reader: untunneled_reader,
                writer: untunneled_writer,
            })
        });
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
}
