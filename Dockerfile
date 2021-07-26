FROM rust:1.53.0-alpine3.13 as base
WORKDIR app
RUN apk add musl-dev && \
    cargo install cargo-chef --version 0.1.22 && \
    rustup component add clippy rustfmt

FROM base as planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM base as cacher
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM base as builder
COPY . .
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo clippy -- -D warnings && \
    cargo fmt --all -- --check && \
    cargo test --locked --offline --all-targets && \
    cargo build --locked --offline --release

FROM alpine:3.13
ARG EXECUTABLE=client
COPY --from=builder /app/target/release/${EXECUTABLE} /app
ENTRYPOINT ["/app"]
