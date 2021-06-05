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

pub(crate) struct DnsTunneler {
    client: Box<dyn AsyncDnsClient>,
}

impl DnsTunneler {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<Self, Box<dyn Error>> {
        let socket = SocketAddr::new(address, port);
        let stream = UdpClientStream::<UdpSocket>::new(socket);
        let (client, background) = AsyncClient::connect(stream).await?;
        tokio::spawn(background);
        Ok(DnsTunneler {
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
        let encoded_data_to_send = hex::encode(&data_to_send[..size]);
        log::debug!("going to send {:?}", encoded_data_to_send);
        let response = self
            .client
            .query(
                Name::from_str(encoded_data_to_send.as_str()).unwrap(),
                DNSClass::IN,
                RecordType::TXT,
            )
            .await?;
        let encoded_received_data = match response.answers()[0].rdata() {
            RData::TXT(text) => format!("{}", text),
            _ => return Err("bla".into()),
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
    use tokio::io;
    use tokio::io::ErrorKind;

    #[tokio::test]
    async fn dns_failed_to_read() -> Result<(), Box<dyn Error>> {
        let mock = MockAsyncDnsClient::new();
        let mut tunneler = DnsTunneler {
            client: Box::new(mock),
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
}
