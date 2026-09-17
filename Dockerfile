# syntax=docker/dockerfile:1

FROM node:26-trixie-slim AS playground-builder
WORKDIR /app/playground
COPY playground/package*.json ./
RUN npm install --no-audit --no-fund
COPY playground ./
RUN npm run build

FROM rust:1-trixie AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY --from=playground-builder /app/playground/dist ./playground/dist
RUN cargo build --release

FROM debian:trixie-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin koeserve

COPY --from=builder /app/target/release/koeserve /usr/local/bin/koeserve

USER koeserve
WORKDIR /app

ENV KOESERVE_ADDR=0.0.0.0:3000 \
    KOESERVE_MODELS_CONFIG=/models/manifest.toml \
    KOESERVE_WORKERS=2 \
    KOESERVE_QUEUE_CAPACITY=32

EXPOSE 3000
ENTRYPOINT ["koeserve"]
CMD ["serve"]
