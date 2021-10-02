use std::error::Error;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::split;
use tokio::net::TcpListener;
use tokio_rustls::rustls::sign::CertifiedKey;
use tokio_rustls::rustls::{
    AllowAnyAuthenticatedClient, ResolvesServerCertUsingSNI, RootCertStore, ServerConfig,
};
use tokio_rustls::TlsAcceptor;

use common::io::Stream;
use common::network::Listener;
use common::tls::{load_certificate, load_signing_key};

use crate::tunnel::{TcpUntunneler, Untunneler};

pub(crate) struct TlsUntunneler {
    untunneler: TcpUntunneler<TlsListener>,
}

impl TlsUntunneler {
    pub(crate) async fn new(
        local_address: IpAddr,
        local_port: u16,
        ca_cert: PathBuf,
        server_cert: PathBuf,
        server_key: PathBuf,
        server_hostname: String,
    ) -> Result<Self, Box<dyn Error>> {
        let mut store = RootCertStore::empty();
        store.add(&load_certificate(ca_cert).await?)?;
        let verifier = AllowAnyAuthenticatedClient::new(store);

        let mut config = ServerConfig::new(verifier);
        let chain = vec![load_certificate(server_cert).await?];
        let mut resolver = ResolvesServerCertUsingSNI::new();
        let signing_key = load_signing_key(server_key).await?;
        resolver.add(
            &server_hostname,
            CertifiedKey::new(chain, Arc::new(signing_key)),
        )?;
        config.cert_resolver = Arc::new(resolver);

        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TlsListener::new(local_address, local_port, acceptor).await?;
        Ok(Self {
            untunneler: TcpUntunneler::from_listener(listener),
        })
    }
}

pub struct TlsListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    pub async fn new(
        local_address: IpAddr,
        local_port: u16,
        acceptor: TlsAcceptor,
    ) -> Result<Self, Box<dyn Error>> {
        let listener_address = format!("{}:{}", local_address, local_port);
        log::info!("start listening on {}", listener_address);
        let listener = TcpListener::bind(listener_address).await?;
        Ok(Self { listener, acceptor })
    }
}

#[async_trait]
impl Listener for TlsListener {
    async fn accept_clients(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        while let Ok((client_stream, client_address)) = self.listener.accept().await {
            log::debug!("got connection from {}", client_address);
            let stream = match self.acceptor.accept(client_stream).await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("failed to accept tls from {}: {}", client_address, e);
                    continue;
                }
            };
            let (client_reader, client_writer) = split(stream);
            new_clients
                .send(Stream::new(client_reader, client_writer))
                .await?;
        }
        Ok(())
    }
}

#[async_trait(?Send)]
impl Untunneler for TlsUntunneler {
    async fn untunnel(&mut self, new_clients: Sender<Stream>) -> Result<(), Box<dyn Error>> {
        self.untunneler.untunnel(new_clients).await
    }
}
