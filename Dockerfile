FROM rust AS builder

RUN apt update && \
    apt install -y pkg-config libssl-dev perl make git

WORKDIR /build

COPY . .

RUN cargo build --release

FROM gcr.io/distroless/cc-debian12
EXPOSE 9100

COPY --from=builder /build/target/release/rcosmos-exporter /

ENTRYPOINT ["./rcosmos-exporter"]