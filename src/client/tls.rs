use std::error::Error;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::split;
use tokio::net::TcpStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::webpki::DNSNameRef;
use tokio_rustls::TlsConnector;

use common::io::Stream;
use common::tls::{load_certificate, load_private_key};

use crate::tunnel::{TcpTunneler, Tunneler};

pub(crate) struct TlsTunneler {
    tunneler: TcpTunneler,
}

impl TlsTunneler {
    pub(crate) async fn new(
        address: IpAddr,
        port: u16,
        ca_cert: PathBuf,
        client_cert: PathBuf,
        client_key: PathBuf,
    ) -> Result<Self, Box<dyn Error>> {
        let mut config = ClientConfig::new();
        config.root_store.add(&load_certificate(ca_cert).await?)?;
        let chain = vec![load_certificate(client_cert).await?];
        config.set_single_client_cert(chain, load_private_key(client_key).await?)?;
        let connector = TlsConnector::from(Arc::new(config));

        let to_address = format!("{}:{}", address, port);
        log::debug!("connecting to {}", to_address);
        let tcp_stream = TcpStream::connect(to_address).await?;

        let domain = DNSNameRef::try_from_ascii_str("server.tunneler")?;
        let stream = connector.connect(domain, tcp_stream).await?;
        let (reader, writer) = split(stream);
        Ok(Self {
            tunneler: TcpTunneler::from_stream(Stream::new(reader, writer))?,
        })
    }
}

#[async_trait]
impl Tunneler for TlsTunneler {
    async fn tunnel(&mut self, client: Stream) -> Result<(), Box<dyn Error>> {
        self.tunneler.tunnel(client).await
    }
}
