use simple_logger::SimpleLogger;
use std::error::Error;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tokio::macros::support::Pin;
use tokio::net::UdpSocket;
use trust_dns_server::authority::MessageResponseBuilder;
use trust_dns_server::proto::op::{Header, MessageType, OpCode};
use trust_dns_server::proto::rr::rdata::TXT;
use trust_dns_server::proto::rr::{DNSClass, RData, Record, RecordType};
use trust_dns_server::server::{Request, RequestHandler, ResponseHandler};
use trust_dns_server::ServerFuture;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()
        .unwrap();
    log::debug!("bla");
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8899));
    let udp_socket = UdpSocket::bind(addr).await?;
    let mut server = ServerFuture::new(A {});
    server.register_socket(udp_socket);
    match server.block_until_done().await {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

struct A {}

fn new_iterator<'a>(
    record: Option<&'a Record>,
) -> Box<dyn Iterator<Item = &'a Record> + Send + 'a> {
    match record {
        None => Box::new(std::array::IntoIter::new([])),
        Some(r) => Box::new(std::array::IntoIter::new([r])),
    }
}

async fn echo<R: ResponseHandler>(request: Request, mut response_handle: R) {
    let queries = request.message.raw_queries();
    let builder = MessageResponseBuilder::new(Option::from(queries));

    let mut response_header = Header::new();
    response_header.set_id(request.message.id());
    response_header.set_op_code(OpCode::Query);
    response_header.set_message_type(MessageType::Response);
    response_header.set_authoritative(true);

    let first_query = (&request.message.queries()[0]).original();
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

impl RequestHandler for A {
    type ResponseFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

    fn handle_request<R: ResponseHandler>(
        &self,
        request: Request,
        response_handle: R,
    ) -> Self::ResponseFuture {
        Box::pin(echo(request, response_handle))
    }
}
