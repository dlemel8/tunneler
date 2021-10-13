# Tunneler
This repo contains client and server that allow you to tunnel TCP and UDP traffic over other network protocols.

Currently, supported tunnels are:
* DNS (authoritative server or direct connection)
* TLS (mutual authentication)
* TCP

Main tool is writen in Rust and end-to-end tests are written in Python.

## Installation
### Option 1: client and server docker images
```sh
docker pull ghcr.io/dlemel8/tunneler-server:latest
docker pull ghcr.io/dlemel8/tunneler-client:latest
```
### Option 2: compile from source code
```sh
cargo --version || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release
```
There is also a [docker file](Dockerfile) if you prefer to build a local docker image

## Usage
### Option 1: client and server docker images
```sh
docker run -e LOCAL_PORT=45301 \
           -e REMOTE_PORT=5201 \
           -e REMOTE_ADDRESS=localhost \
           -e TUNNELED_TYPE=udp \
           --rm -p 45301:45301 ghcr.io/dlemel8/tunneler-server:latest tcp
```
### Option 2: locally compiled binary
```sh
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
TUNNELED_TYPE=tcp \
LOG_LEVEL=debug \
./target/debug/client 1.1.1.1 53 dns
```
Run docker image or compiled binary with `--help` for more information

## Examples
This repo contains a few server deployment examples using Docker Compose:
* Speed Test - iperf3 server exposed via all supported tunnels, allow you to compare tunnels speed.
* Authoritative DNS - Redis server exposed via DNS tunnel on port UDP/53. Since tunnel does not verify clients, Redis
  authentication is needed.
* Pipeline - Redis server exposed via TLS tunnel that itself exposed via DNS tunnel. on port UDP/53. Since tunnel does 
  verify clients, Redis authentication is not used.

You can run each example locally or deploy it using Terraform and Ansible. See more information [here](examples/README.md).

## Architecture
![Architecture](architecture.jpg?raw=true "Architecture")
Each executable contains 2 components communicating via a channel of client streams (a tuple of bytes reader and writer):
* Client Listener binds a socket and convert incoming and outgoing traffic to a new stream.
* Client Tunneler translate stream reader and writer to the tunnel protocol.
* Server Untunneler binds a socket according to tunnel protocol and translate tunneled traffic back to original stream.
* Server Forwarder converts stream writer and reader back to traffic. 

TCP based traffic is trivially converted to stream. UDP based traffic conversion depends on the tunnel protocol. 

UDP based traffic also need a way to identify existing clients to continue their sessions. The solution is an in-memory 
Clients Cache that maps an identifier of the client to its stream.

## Protocols
### Tunneled UDP
In order to convert UDP traffic to a stream, a 2 bytes size header (in big endian) precedes each packet payload.

UDP Listener uses incoming packet peer address as a key to its Clients Cache. 

### DNS tunneling
We have a few challenges here:
* DNS payload must be alphanumeric.
* Every message requires a response before we can send next message.
* Each DNS query uses randomized source port and transaction ID, so we can't use them as Client Cache key.

To solve those challenges, each client session starts with generating a random Client ID (4 alphanumeric chars). Client 
reads data to tunnel and run it via a pipeline of encoders: 
* Data is encoded in hex. 
* Client ID is appended.
* Client suffix appended. In case the server is running on your authoritative DNS server, suffix is ".\<your domain>".

Encoded data is then used as the name of a TXT DNS query.

If you own an authoritative DNS server, client can send the request to a recursive DNS resolver. Resolver will get your 
IP from your domain name registrar and forward the request to your IP. Another option (faster but more obvious to any 
traffic analyzer) is configuring client to send the request directly to your IP (on port UDP/53 or any other port server 
is listening to).

Server decodes data (ignoring any non client traffic) and uses Client ID as a key to its Clients Cache.

In order to handle large server responses and empty TCP ACKs, read timeout is used in both client and server. If read 
timeout is expired, empty message will be sent. Both client and server use idle timeout to stop forwarding and cleanup 
local resources.

### TLS tunneling
To implement mutual authentication, we use a private Certificate Authority:
* A self-signed certificate is generated from a private key.
* Server private key is randomly generated and its public part is embedded in a certificate signed by the self-signed 
  certificate.
* Client private key is randomly generated and its public part is embedded in a certificate signed by the self-signed
  certificate.

Both client and server are configured to use their key and certificate in TLS handshake. The self-signed certificate is 
used as TLS trust root.

Since Server Name Indication extension is used, client is requesting a specific server name and server is serving its 
certificate only if that name was requested. The server name must also be part of the certificate, for example as a 
Subject Alternative Name.

## Testing
### Run Unit Tests
```sh
cargo test --all-targets
```
### Run End-to-End Tests
```sh
python3 -m pip install -r e2e_tests/requirements.txt
PYTHONPATH=. python3 -m pytest -v
```
