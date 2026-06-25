FROM rust:1.96-slim AS builder

WORKDIR /usr/src/iptv-rs
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim

ARG APP_WORKDIR=/iptv-rs
ENV APP_WORKDIR=${APP_WORKDIR}
ENV CONFIG_PATH=${APP_WORKDIR}/config/config.ini
ENV NGINX_HTTP_PORT=8080
ENV PUBLIC_SCHEME=http
ENV PUBLIC_DOMAIN=127.0.0.1
ENV PUBLIC_PORT=80
ENV UPDATE_STARTUP=true
ENV UPDATE_INTERVAL=12

WORKDIR ${APP_WORKDIR}
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/iptv-rs/target/release/iptv-rs /usr/local/bin/iptv-rs
COPY config /iptv-rs-config
COPY docker/entrypoint.sh /iptv-rs-entrypoint.sh
RUN chmod +x /iptv-rs-entrypoint.sh \
    && mkdir -p ${APP_WORKDIR}/config ${APP_WORKDIR}/output

VOLUME ["/iptv-rs/config", "/iptv-rs/output"]
EXPOSE 8080

ENTRYPOINT ["/iptv-rs-entrypoint.sh"]
