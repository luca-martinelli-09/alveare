FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release -p panel

FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata mariadb-client restic \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/panel /usr/local/bin/panel
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --retries=3 CMD ["panel", "healthcheck"]
CMD ["panel"]
