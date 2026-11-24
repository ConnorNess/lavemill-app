# syntax=docker/dockerfile:1
FROM rust:1.90.0-bookworm AS development

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl git pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown \
    && rustup component add rustfmt clippy

# Keep dx outside the source mount and Cargo download volumes.
RUN cargo install dioxus-cli --version 0.7.3 --locked --root /opt/dioxus --jobs 2

ENV PATH="/opt/dioxus/bin:${PATH}" \
    CARGO_TARGET_DIR=/app/target \
    CARGO_BUILD_JOBS=2

RUN apt-get update \
    && apt-get install -y --no-install-recommends sqlite3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
RUN mkdir -p /data
EXPOSE 8080

CMD ["dx", "serve", "--web", "--addr", "0.0.0.0", "--port", "8080", "--open", "false", "--interactive", "false", "--exit-on-error"]
