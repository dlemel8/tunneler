use std::array::IntoIter;
use std::future::Future;
use std::pin::Pin;

use trust_dns_client::op::{Header, MessageType, OpCode};
use trust_dns_client::proto::rr::{DNSClass, RData, Record, RecordType};
use trust_dns_client::proto::rr::rdata::TXT;
use trust_dns_server::authority::MessageResponseBuilder;
use trust_dns_server::server::{Request, RequestHandler, ResponseHandler};

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

pub struct EchoRequestHandler {}

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
