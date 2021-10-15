# Speed Test
This example contains iperf3 server exposed via all supported tunnels:
* TCP over TCP, exposed on port 45301.
* UDP over TCP, exposed on port 45302.
* TCP over DNS, exposed on port 53535.
* UDP over DNS, exposed on port 53536.
* TCP over TLS, exposed on port 44301.
* UDP over TLS, exposed on port 44302.

In addition, in development mode, iperf3 server itself exposed on port 45201 (TCP + UDP).

## Terraform Variables
* CA_PRIVATE_KEY - private key (for example, SSH RSA) that signs CA certificate.
* CA_CERTIFICATE - certificate that will sign server certificate (and should be used as CA_CERT of Tunneler client).

## Client
After you deploy server, you can run iperf3 client via one or more tunnels.

### TCP Speed Test
iperf3 TCP test connects to a single TCP port. 

First, run selected tunnel, for example:
```sh
LOCAL_PORT=8888 \
REMOTE_PORT=44301 \
REMOTE_ADDRESS=127.0.0.1 \
LOG_LEVEL=debug \
TUNNELED_TYPE=tcp \
CA_CERT=../../pki/ca.crt \
CERT=../../pki/client.crt \
KEY=../../pki/client.key \
SERVER_HOSTNAME=server.tunneler \
../../target/release/client tls
```

Then, run iperf3 client and direct it to the tunnel local port, for example:
```sh
iperf3 -c 127.0.0.1 -p 8888
```

### UDP Speed Test
iperf3 UDP test connects to the same port, both TCP and UDP. 

First, run selected tunnels with the same local port, for example:
```sh
LOCAL_PORT=8888 \
REMOTE_PORT=53535 \
REMOTE_ADDRESS=127.0.0.1 \
TUNNELED_TYPE=tcp \
LOG_LEVEL=debug \
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
../../target/release/client dns

LOCAL_PORT=8888 \
REMOTE_PORT=53536 \
REMOTE_ADDRESS=127.0.0.1 \
TUNNELED_TYPE=udp \
LOG_LEVEL=debug \
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
../../target/release/client dns
```

Then, run iperf3 client and direct it to the tunnels local port, for example:
```sh
iperf3 -c 127.0.0.1 -p 8888 -u
```
