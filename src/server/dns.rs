use std::array::IntoIter;
use std::error::Error;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::{duplex, split};
use tokio::net::UdpSocket;
use trust_dns_client::op::{Header, MessageType, OpCode};
use trust_dns_client::proto::rr::rdata::TXT;
use trust_dns_client::proto::rr::{DNSClass, RData, Record, RecordType};
use trust_dns_server::authority::MessageResponseBuilder;
use trust_dns_server::server::{Request, RequestHandler, ResponseHandler};
use trust_dns_server::ServerFuture;

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

pub struct UntunnelRequestHandler<F: StreamCreator> {
    untunneled_clients: Arc<StreamsCache<F, SocketAddr>>,
}

impl<F: StreamCreator> UntunnelRequestHandler<F> {
    pub(crate) fn new(cache: StreamsCache<F, SocketAddr>) -> Self {
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
            response_handle,
            self.untunneled_clients.clone(),
        ))
    }
}

async fn untunnel_request<R: ResponseHandler, F: StreamCreator>(
    request: Request,
    mut response_handle: R,
    untunneled_clients: Arc<StreamsCache<F, SocketAddr>>,
) {
    let message = request.message;
    let builder = MessageResponseBuilder::new(Option::from(message.raw_queries()));

    let mut response_header = Header::new();
    response_header.set_id(message.id());
    response_header.set_op_code(OpCode::Query);
    response_header.set_message_type(MessageType::Response);
    response_header.set_authoritative(true);

    let first_query = (&message.queries()[0]).original();
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
    let received_data = encoded_received_data.as_bytes();
    let client = untunneled_clients.get(request.src).unwrap();
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

    let encoded_data_to_tunnel = String::from_utf8_lossy(&data_to_tunnel[..size]).into();
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
