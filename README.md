# Tunneler
This repo contains client and server that allow you to tunnel TCP traffic via other network protocols.

Currently, supported tunnels are:
* DNS tunneling
* TCP proxy

Main tool is writen in Rust and end-to-end tests are written in Python.

## Installation
### Option 1: client and server docker images
```sh
docker pull ghcr.io/dlemel8/tunneler-server:main
docker pull ghcr.io/dlemel8/tunneler-client:main
```
### Option 2: compile from source code
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release
```
There is also a [docker file](Dockerfile) if you prefer to build a local docker image

## Usage
### Option 1: client and server docker images
```sh
docker run -e LOCAL_PORT=45301 \
           -e REMOTE_PORT=5201 \
           -e REMOTE_ADDRESS=localhost \
           --rm -p 45301:45301 ghcr.io/dlemel8/tunneler-server:main tcp
```
### Option 2: locally compiled binary
```sh
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
LOG_LEVEL=debug \
./target/debug/client 127.0.0.1 53 dns
```
Run docker image or compiled binary with `--help` for more information

## Deployment Example
This repo contains a [docker compose file](docker-compose.server.yml) with the following services:
* speed test, based on iperf3 server, exporting port tcp/45201 to host
* speed test tcp proxy, based on tunneler server, exporting port tcp/45301 to host
* speed test dns tunnel, based on tunneler server, exporting port udp/53 to host

## Architecture
![Architecture](images/architecture.jpg?raw=true "Architecture")

## Protocols
### DNS tunneling
We have a few challenges here:
* DNS Untunneler translates UDP packets to TCP, so we need a way to identify existing clients to continue their sessions.
* DNS payload must be alphanumeric.
* Every message requires a response before we can send next message.

To solve those challenges, each client session starts with generating a random Client ID. Client reads data to tunnel 
and run it via a composition of encoders: 
* Data is encoded in hex. 
* Client ID is appended to encoded data.

Final data is then embedded in a DNS query name, requesting TXT record.

Server extracts original data and get or create Client ID in an in-memory Clients Cache. Clients Cache maps Client ID 
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
