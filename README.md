# Tunneler
This repo contains client and server that allow you to tunnel TCP traffic via other network protocols.

Currently, supported tunnels are:
* DNS tunneling (authoritative DNS server or direct connection)
* TCP proxy

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
           --rm -p 45301:45301 ghcr.io/dlemel8/tunneler-server:latest tcp
```
### Option 2: locally compiled binary
```sh
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
LOG_LEVEL=debug \
./target/debug/client 127.0.0.1 53 dns
```
Run docker image or compiled binary with `--help` for more information

## Examples
This repo contains a few server deployment examples using Docker Compose:
* Speed Test - iperf3 server exposed via all supported tunnels, allow you to compare tunnels speed.
* Authoritative DNS - Redis server exposed via DNS tunnel on port UDP/53. Since tunnel is not verify client, Redis 
authentication is needed.

You can run each example locally or deploy it using Terraform and Ansible. See more information [here](examples/README.md).

## Architecture
![Architecture](images/architecture.jpg?raw=true "Architecture")

## Protocols
### DNS tunneling
We have a few challenges here:
* DNS Untunneler translates UDP packets to TCP, so we need a way to identify existing clients to continue their sessions.
* DNS payload must be alphanumeric.
* Every message requires a response before we can send next message.

To solve those challenges, each client session starts with generating a random Client ID. Client reads data to tunnel 
and run it via a pipeline of encoders: 
* Data is encoded in hex. 
* Client ID is appended.
* Client suffix appended. In case the server is running on your authoritative DNS server, suffix is ".\<your domain>".

Encoded data is then used as the name of a TXT DNS query.

If you own an authoritative DNS server, client can send the request to a recursive DNS resolver. Resolver will get your 
IP from your domain name registrar and forward the request to your IP. Another option (faster but more obvious to any 
traffic analyzer) is configuring client to send the request directly to your IP (on port UDP/53 or any other port server 
is listening to).

Server decodes data (ignoring any non client traffic) and get or create Client ID in an in-memory Clients Cache. Clients Cache maps Client ID 
into in-memory reader and writer. Original data is then forwarded using TCP to target service.

In order to handle large server responses and empty TCP ACKs, read timeout is used in both client and server. If read 
timeout is expired, empty message will be sent. Both client and server use idle timeout to stop forwarding and cleanup 
local resources.

### TCP proxy
Currently, only TCP listener and forwarder are supported, so nothing is special here.


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
