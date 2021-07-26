FROM rust:1.53.0-alpine3.13 as builder

RUN apk add musl-dev && rustup component add clippy rustfmt

WORKDIR /usr/src/

# dummy build to cache dependencies
RUN cargo init --lib && \
    mkdir src/common && \
    mv src/lib.rs src/common
COPY Cargo.lock Cargo.toml ./
RUN cargo build --release --lib

COPY src src
RUN cargo clippy -- -D warnings && \
    cargo fmt --all -- --check && \
    cargo test --all-targets && \
    touch src/common/lib.rs && cargo build --release

FROM alpine:3.13

ARG EXECUTABLE=client
COPY --from=builder /usr/src/target/release/${EXECUTABLE} /app

ENTRYPOINT ["/app"]
