FROM rust:1.96-alpine AS builder

WORKDIR /usr/src/iptv-rs
RUN apk add --no-cache build-base
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM scratch

ARG APP_WORKDIR=/iptv-rs
ENV APP_WORKDIR=${APP_WORKDIR}
ENV CONFIG_PATH=${APP_WORKDIR}/config/config.ini
ENV IPTV_RS_DEFAULT_CONFIG_DIR=/iptv-rs-config
ENV NGINX_HTTP_PORT=8080
ENV PUBLIC_SCHEME=http
ENV PUBLIC_DOMAIN=127.0.0.1
ENV PUBLIC_PORT=80
ENV UPDATE_STARTUP=true
ENV UPDATE_INTERVAL=12

WORKDIR ${APP_WORKDIR}
COPY --from=builder /usr/src/iptv-rs/target/release/iptv-rs /usr/local/bin/iptv-rs
COPY config /iptv-rs-config

VOLUME ["/iptv-rs/config", "/iptv-rs/output"]
EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/iptv-rs"]
CMD ["serve"]
