use crate::tunnel::{TcpUntunneler, Untunneler};
use async_channel::Receiver;
use common::io::{copy, AsyncReadWrapper, AsyncReader, AsyncWriteWrapper, AsyncWriter, Stream};
use simple_logger::SimpleLogger;
use std::array::IntoIter;
use std::error::Error;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::macros::support::Pin;
use tokio::net::{TcpStream, UdpSocket};
use trust_dns_server::authority::MessageResponseBuilder;
use trust_dns_server::proto::op::{Header, MessageType, OpCode};
use trust_dns_server::proto::rr::rdata::TXT;
use trust_dns_server::proto::rr::{DNSClass, RData, Record, RecordType};
use trust_dns_server::server::{Request, RequestHandler, ResponseHandler};
use trust_dns_server::ServerFuture;

mod tunnel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()
        .unwrap();
    log::debug!("start server");

    tcp_server().await?;
    dns_server().await
}

async fn dns_server() -> Result<(), Box<dyn Error>> {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8899));
    let udp_socket = UdpSocket::bind(addr).await?;
    let mut server = ServerFuture::new(EchoRequestHandler {});
    server.register_socket(udp_socket);
    match server.block_until_done().await {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn forward_clients(
    clients: Receiver<Stream>,
    remote_address: IpAddr,
    remote_port: u16,
) -> Result<(), Box<dyn Error>> {
    while let Ok(mut client) = clients.recv().await {
        let to_address = format!("{}:{}", remote_address, remote_port);
        log::debug!("connecting to {}", to_address);
        let stream = TcpStream::connect(to_address).await?;
        let (to_reader, to_writer) = stream.into_split();
        let mut to_tunnel: Box<dyn AsyncReader> = Box::new(AsyncReadWrapper::new(to_reader));
        let mut from_tunnel: Box<dyn AsyncWriter> = Box::new(AsyncWriteWrapper::new(to_writer));
        let to_tunnel_future = copy(&mut to_tunnel, &mut client.writer, "sending to tunnel");
        let from_tunnel_future = copy(&mut client.reader, &mut from_tunnel, "received from tunnel");
        tokio::try_join!(to_tunnel_future, from_tunnel_future)?;
    }
    Ok(())
}

async fn tcp_server() -> Result<(), Box<dyn Error>> {
    let mut untunneler = TcpUntunneler::new("127.0.0.1".parse()?, 8899).await?;
    let (untunneled_sender, untunneled_receiver) = async_channel::unbounded::<Stream>();
    let untunnel_clients_future = untunneler.untunnel(untunneled_sender);
    let forward_clients_future = forward_clients(untunneled_receiver, "127.0.0.1".parse()?, 8080);
    match tokio::try_join!(untunnel_clients_future, forward_clients_future) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

fn new_iterator<'a>(
    record: Option<&'a Record>,
) -> Box<dyn Iterator<Item = &'a Record> + Send + 'a> {
    match record {
        None => Box::new(IntoIter::new([])),
        Some(r) => Box::new(IntoIter::new([r])),
    }
}

async fn echo<R: ResponseHandler>(request: Request, mut response_handle: R) {
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

    log::debug!("received {:?}", encoded_received_data);
    let answer = Record::new()
        .set_rdata(RData::TXT(TXT::new(vec![encoded_received_data])))
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

struct EchoRequestHandler {}

impl RequestHandler for EchoRequestHandler {
    type ResponseFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

    fn handle_request<R: ResponseHandler>(
        &self,
        request: Request,
        response_handle: R,
    ) -> Self::ResponseFuture {
        Box::pin(echo(request, response_handle))
    }
}
