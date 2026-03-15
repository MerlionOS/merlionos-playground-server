# ---------- builder ----------
FROM rust:latest AS builder

RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .
RUN cargo build --release

# ---------- runtime ----------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates qemu-system-x86 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/merlionos-playground-server /usr/local/bin/
COPY images/ /app/images/

WORKDIR /app
ENV KERNEL_IMAGE=/app/images/merlionos.bin

EXPOSE 3020

CMD ["merlionos-playground-server"]
