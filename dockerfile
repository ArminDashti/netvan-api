FROM rust:1.88-bookworm

RUN apt-get update \
  && apt-get install -y --no-install-recommends pkg-config libssl-dev iputils-ping traceroute iproute2 ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && cargo install cargo-watch --locked

WORKDIR /app

ENV NETVAN_API_BIND=0.0.0.0:8000
ENV NETVAN_API_DATA_DIR=/data
ENV CARGO_TERM_COLOR=always
ENV CARGO_TARGET_DIR=/app/target
ENV CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu

EXPOSE 8000

CMD ["cargo", "watch", "-w", "crates", "-w", "Cargo.toml", "-w", "Cargo.lock", "--poll", "-x", "run -p netvan-api -- run"]
