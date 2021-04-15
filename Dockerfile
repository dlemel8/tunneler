FROM rust:1.51.0-alpine3.13 as builder

RUN apk add musl-dev && rustup component add clippy rustfmt

ARG EXECUTABLE=client

WORKDIR /usr/src/
RUN cargo init ${EXECUTABLE}
WORKDIR /usr/src/${EXECUTABLE}

COPY ${EXECUTABLE}/Cargo.lock ${EXECUTABLE}/Cargo.toml ./
# This is a dummy build to get the dependencies cached.
RUN cargo build --release

COPY ${EXECUTABLE}/src src
RUN cargo clippy -- -D warnings && \
    cargo fmt --all -- --check && \
    cargo test --all-targets && \
    touch -a -m ./src/main.rs && cargo build --release

FROM alpine:3.13

ARG EXECUTABLE=client
COPY --from=builder /usr/src/${EXECUTABLE}/target/release/${EXECUTABLE} /app

ENTRYPOINT ["/app"]
CMD ["--help"]