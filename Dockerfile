# ---- Build stage: compile musl static binary ----
# Alpine is musl-based, so a native release build produces a static musl binary.
FROM rust:1.85-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build

# Cache dependency compilation separately from source changes
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Build actual source
COPY src ./src
RUN touch src/main.rs && \
    cargo build --release && \
    cp target/release/gluetun-connector /binary

# ---- Final stage: minimal scratch image ----
FROM scratch

LABEL org.opencontainers.image.source="https://github.com/krabhi4/gluetun-connector"
LABEL org.opencontainers.image.description="Web UI and Monitor for Gluetun VPN container management"
LABEL org.opencontainers.image.licenses="MIT"

COPY --from=builder /binary /usr/local/bin/gluetun-connector
COPY public/ /app/public/

WORKDIR /app

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD ["/usr/local/bin/gluetun-connector", "--health-check"]

CMD ["/usr/local/bin/gluetun-connector"]
