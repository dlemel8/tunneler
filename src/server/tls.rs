use std::error::Error;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use tokio::io::split;
use tokio::net::TcpListener;
use tokio_rustls::rustls::{AllowAnyAuthenticatedClient, RootCertStore, ServerConfig};
use tokio_rustls::TlsAcceptor;

use common::io::Stream;
use common::network::Listener;

use crate::tunnel::{untunnel_clients, Untunneler};
use common::tls::{load_certificate, load_private_key};

pub(crate) struct TlsUntunneler {
    server: TlsListener,
}

impl TlsUntunneler {
    pub(crate) async fn new(
        local_address: IpAddr,
        local_port: u16,
        ca_cert: PathBuf,
        server_cert: PathBuf,
        server_key: PathBuf,
    ) -> Result<Self, Box<dyn Error>> {
        let mut store = RootCertStore::empty();
        store.add(&load_certificate(ca_cert).await?)?;
        let verifier = AllowAnyAuthenticatedClient::new(store);

        let mut config = ServerConfig::new(verifier);
        let chain = vec![load_certificate(server_cert).await?];
        config.set_single_cert(chain, load_private_key(server_key).await?)?;

        let acceptor = TlsAcceptor::from(Arc::new(config));
        let server = TlsListener::new(local_address, local_port, acceptor).await?;
        Ok(Self { server })
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
        let (tunneled_sender, tunneled_receiver) = async_channel::unbounded::<Stream>();
        let accept_clients_future = self.server.accept_clients(tunneled_sender);
        let untunnel_clients_future = untunnel_clients(tunneled_receiver, new_clients);
        match tokio::try_join!(untunnel_clients_future, accept_clients_future) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}
