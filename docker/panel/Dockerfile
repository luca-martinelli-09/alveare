FROM node:24-slim AS ui
WORKDIR /ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY ui ./
RUN npm run build

FROM rust:1-slim-trixie AS build
# OpenSSL per webauthn-rs (passkey).
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY --from=ui /ui/build ui/build
# dev-<commit> o il tag della release, dalla CI (vuoto in locale).
ARG ALVEARE_BUILD
RUN ALVEARE_BUILD=${ALVEARE_BUILD} cargo build --release -p panel

FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata mariadb-client restic libssl3t64 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/panel /usr/local/bin/panel
# Copiati in /srv/templates al primo avvio se la cartella è vuota (compose senza il repo).
COPY templates/wordpress /usr/share/alveare/templates/wordpress
COPY templates/laravel /usr/share/alveare/templates/laravel
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --retries=3 CMD ["panel", "healthcheck"]
CMD ["panel"]
