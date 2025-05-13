FROM rust:slim-bookworm AS builder

RUN apt update && \
    apt install -y pkg-config libssl-dev perl make git

WORKDIR /build

COPY . .

RUN cargo build --release

FROM gcr.io/distroless/cc-debian12

ENV LOGGING_LEVEL=DEBUG
ENV PROMETHEUS_IP=0.0.0.0
ENV BLOCK_WINDOW=500

EXPOSE 9100

COPY --from=builder /build/target/release/rcosmos-exporter /

ENTRYPOINT ["./rcosmos-exporter"]