use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use async_trait::async_trait;
use tokio::net::UdpSocket;
use trust_dns_client::client::{AsyncClient, ClientHandle};
use trust_dns_client::proto::rr::Name;
use trust_dns_client::rr::{DNSClass, RData, RecordType};
use trust_dns_client::udp::UdpClientStream;

use crate::tunnel::{AsyncReader, AsyncWriter, Tunneler};

pub(crate) struct DnsTunnel {
    client: AsyncClient,
}

impl DnsTunnel {
    pub(crate) async fn new(address: IpAddr, port: u16) -> Result<DnsTunnel, Box<dyn Error>> {
        let socket = SocketAddr::new(address, port);
        let stream = UdpClientStream::<UdpSocket>::new(socket);
        let (client, background) = AsyncClient::connect(stream).await?;
        tokio::spawn(background);
        Ok(DnsTunnel { client })
    }
}

#[async_trait]
impl Tunneler for DnsTunnel {
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
