# Authoritative DNS
This example contains Redis server exposed via TCP over DNS tunnel:
* In production mode, DNS tunnel is exposed on external IP (port UDP/53) and Redis server is password protected
* In development mode, DNS tunnel is exposed on localhost (port UDP/5333) and Redis server is not password protected

## Terraform Variables
* REDIS_PASSWORD - Redis server authentication password.

## Client
After you deploy server, you can run Redis client via a DNS tunnel.

### Direct Connection
First, run tunnel with server IP as `REMOTE_ADDRESS`, for example:
```sh
LOCAL_PORT=8888 \
REMOTE_PORT=5333 \
REMOTE_ADDRESS=<your server IP> \
TUNNELED_TYPE=tcp \
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
LOG_LEVEL=debug \
../../target/release/client dns
```

Then, run Redis client and direct it to the tunnel local port, for example:
```sh
docker run --net=host --rm -it redis:6.0.12-alpine redis-cli -p 8888 info
```

### Authoritative DNS server
First, run tunnel with a DNS resolver IP as `REMOTE_ADDRESS` and your domain as `CLIENT_SUFFIX`, for example:
```sh
LOCAL_PORT=8888 \
REMOTE_PORT=53 \
REMOTE_ADDRESS=1.1.1.1 \
TUNNELED_TYPE=tcp \
CLIENT_SUFFIX=.<your authoritative server domain> \
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
../../target/release/client dns
```

Then, run Redis client and direct it to the tunnel local port, for example:
```sh
docker run --net=host --rm -it redis:6.0.12-alpine redis-cli -p 8888 -a <redis password> info
```
