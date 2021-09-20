use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use tokio::net::{TcpStream, UdpSocket};

use common::io::{copy, AsyncReader, AsyncWriter, Stream};
use common::network::{
    stream_udp_packet, unstream_udp_packet, UnstreamPacketResult, MAX_UDP_PACKET_SIZE,
};

pub(crate) async fn forward_client_tcp(
    mut client: Stream,
    remote_address: IpAddr,
    remote_port: u16,
) {
    let to_address = format!("{}:{}", remote_address, remote_port);
    log::debug!("connecting to {}", to_address);
    let server = match TcpStream::connect(&to_address).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("failed to connect to {}: {}", to_address, e);
            return;
        }
    };

    let (server_reader, server_writer) = server.into_split();
    let mut server_stream = Stream::new(server_reader, server_writer);
    let to_tunnel_future = copy(
        &mut server_stream.reader,
        &mut client.writer,
        "sending to tunnel",
    );
    let from_tunnel_future = copy(
        &mut client.reader,
        &mut server_stream.writer,
        "received from tunnel",
    );
    if let Err(e) = tokio::try_join!(to_tunnel_future, from_tunnel_future) {
        log::error!("failed to forward client: {}", e)
    }
}

pub(crate) async fn forward_client_udp(client: Stream, remote_address: IpAddr, remote_port: u16) {
    let bind_addr = "0.0.0.0:0"; // let OS choose local IP and port
    let socket = match UdpSocket::bind(&bind_addr).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("failed to bind udp socket: {}", e);
            return;
        }
    };

    let to_address = format!("{}:{}", remote_address, remote_port);
    log::debug!("connecting to {}", to_address);
    if let Err(e) = socket.connect(&to_address).await {
        log::error!("failed to connect udp socket to {}: {}", to_address, e);
        return;
    }

    let socket = Arc::new(socket);
    let to_tunnel_future = stream_udp(client.writer, socket.clone());
    let from_tunnel_future = unstream_udp(client.reader, socket, to_address.parse().unwrap());
    tokio::join!(to_tunnel_future, from_tunnel_future);
}

async fn stream_udp(mut writer: Box<dyn AsyncWriter>, socket: Arc<UdpSocket>) {
    let mut data = vec![0; MAX_UDP_PACKET_SIZE];
    while let Ok(size) = socket.recv(&mut data).await {
        stream_udp_packet(&mut data, size, &mut writer).await;
    }
}

async fn unstream_udp(
    mut reader: Box<dyn AsyncReader>,
    socket: Arc<UdpSocket>,
    target: SocketAddr,
) {
    while let UnstreamPacketResult::Payload(data) = unstream_udp_packet(&mut reader, None).await {
        log::debug!("sending to {} received data {:?}", target, data);
        if let Err(e) = socket.send_to(&data, &target).await {
            log::error!("failed to sending  data to {}: {}", target, e);
        };
    }
}
