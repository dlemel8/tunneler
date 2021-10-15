# Speed Test
This example contains Redis server exposed via TCP over TLS tunnel, that itself exposed via TCP over DNS tunnel:
* DNS tunnel is exposed on port UDP/56379
* In production mode, multiple docker compose networks are used to make redis inaccessible from DNS tunnel container
* In development mode, Redis is exposed on port TCP/6379 and TLS tunnel is exposed on port TLS/46379

## Terraform Variables
* CA_PRIVATE_KEY - private key (for example, SSH RSA) that signs CA certificate.
* CA_CERTIFICATE - certificate that will sign server certificate (and should be used as CA_CERT of Tunneler client).

## Client
After you deploy server, you can run Redis client via a TLS tunnel and TLS tunnel via a DNS tunnel.

First, run both tunnel in a pipeline, for example:
```sh
LOCAL_PORT=8886 \
REMOTE_PORT=56379 \
REMOTE_ADDRESS=127.0.0.1 \
LOG_LEVEL=debug \
TUNNELED_TYPE=tcp \
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
../../target/release/client dns

LOCAL_PORT=8887 \
REMOTE_PORT=8886 \
REMOTE_ADDRESS=127.0.0.1 \
LOG_LEVEL=debug \
TUNNELED_TYPE=tcp \
CA_CERT=../../pki/ca.crt \
CERT=../../pki/client.crt \
KEY=../../pki/client.key \
SERVER_HOSTNAME=server.tunneler \
../../target/release/client tls
```

Then, run Redis client and direct it to the TLS tunnel local port, for example:
```sh
docker run --net=host --rm -it redis:6.0.12-alpine redis-cli -p 8887 info
```
