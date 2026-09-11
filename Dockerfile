FROM rust:1.96.1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p rustsync-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 rustsync \
    && useradd --uid 10001 --gid rustsync --no-create-home --shell /usr/sbin/nologin rustsync \
    && mkdir -p /data \
    && chown rustsync:rustsync /data
COPY --from=builder /build/target/release/rustsync-server /usr/local/bin/rustsync-server
ENV RUSTSYNC_HOST=0.0.0.0 \
    RUSTSYNC_PORT=3000 \
    RUSTSYNC_STORAGE_DIR=/data
USER 10001:10001
EXPOSE 3000
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:3000/health || exit 1
ENTRYPOINT ["rustsync-server"]
