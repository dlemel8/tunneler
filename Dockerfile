FROM rust:1.53.0-alpine3.13 as builder

RUN apk add musl-dev && rustup component add clippy rustfmt

WORKDIR /usr/src/

COPY . .
RUN cargo clippy -- -D warnings && \
    cargo fmt --all -- --check && \
    cargo test --all-targets && \
    touch src/client/main.rs src/common/lib.rs src/client/main.rs && cargo build --release

FROM alpine:3.13

ARG EXECUTABLE=client
COPY --from=builder /usr/src/target/release/${EXECUTABLE} /app

ENTRYPOINT ["/app"]
