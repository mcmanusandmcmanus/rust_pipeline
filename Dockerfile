# syntax=docker/dockerfile:1.7

FROM rust:1.82 as builder
WORKDIR /app

# Pre-build dependency layer for faster incremental builds.
COPY webapp/Cargo.toml webapp/Cargo.lock ./webapp/
RUN mkdir -p webapp/src && echo "fn main() {}" > webapp/src/main.rs
RUN cd webapp && cargo fetch

# Actual sources.
COPY webapp ./webapp
RUN cd webapp && cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home appuser
WORKDIR /app
COPY --from=builder /app/webapp/target/release/webapp .

ENV WEBAPP_HOST=0.0.0.0
# Render injects $PORT; we default to 10000 for local Docker runs.
ENV WEBAPP_PORT=10000
ENV RUST_LOG=info

EXPOSE 10000
USER appuser
CMD ["./webapp"]
