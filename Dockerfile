FROM rust:slim-bookworm AS builder

RUN apt update && \
    apt install -y pkg-config libssl-dev

WORKDIR /build

COPY . .

RUN cargo build --release

FROM debian:bookworm-slim

ENV LOGGING_LEVEL=DEBUG
ENV PROMETHEUS_IP=0.0.0.0
ENV BLOCK_WINDOW=500

EXPOSE 9100

RUN apt update && \
    apt install -y ca-certificates pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/rcosmos_exporter /usr/local/bin/

WORKDIR /usr/local/bin/

ENTRYPOINT ["/usr/local/bin/rcosmos_exporter"]