# Immagine PHP condivisa da tutti i siti: un pool FPM per sito, generato dal pannello.
# PHP_VERSION=8.4 per un runtime aggiuntivo (servizio php84 del compose).
ARG PHP_VERSION=8.5
FROM rust:1-slim-trixie AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

# Ricetta delle dipendenze: cambia solo con i Cargo.toml e il Cargo.lock, non con il codice.
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS agent
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release -p php-agent --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release -p php-agent

FROM php:${PHP_VERSION}-fpm
ARG WP_CLI_VERSION=2.12.0
ARG ADMINER_VERSION=6.2.0
ARG ADMINER_SHA256=7492bf2b158fee5b7ddc9e13255322c4601b1f51b797896e303882626bf1e5af

COPY --from=mlocati/php-extension-installer:2 /usr/bin/install-php-extensions /usr/local/bin/
# Composer per il deploy delle app Laravel (composer install con l'utente del sito).
COPY --from=composer:2 /usr/bin/composer /usr/local/bin/composer
RUN install-php-extensions gd intl zip mysqli pdo_mysql bcmath exif redis pcntl \
 && apt-get update && apt-get install -y --no-install-recommends mariadb-client less git openssh-client unzip \
 && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /usr/local/bin/wp \
      "https://github.com/wp-cli/wp-cli/releases/download/v${WP_CLI_VERSION}/wp-cli-${WP_CLI_VERSION}.phar" \
 && chmod 0755 /usr/local/bin/wp

RUN mkdir -p /usr/share/adminer \
 && curl -fsSL -o /usr/share/adminer/adminer.php \
      "https://github.com/vrana/adminer/releases/download/v${ADMINER_VERSION}/adminer-${ADMINER_VERSION}-mysql.php" \
 && echo "${ADMINER_SHA256}  /usr/share/adminer/adminer.php" | sha256sum -c -
COPY docker/php/adminer/index.php /usr/share/adminer/index.php

# Via i pool di default (www, porta 9000): esistono solo i pool dei siti.
RUN rm -f /usr/local/etc/php-fpm.d/*.conf /usr/local/etc/php-fpm.d/*.default \
 && mkdir -p /srv/php/pools /srv/php/staging /run/php /run/alveare
# Utenti dei siti generati dal pannello in /srv/php/etc: con un link (e non un bind mount
# del singolo file) la sostituzione atomica del pannello si vede subito.
RUN ln -sf /srv/php/etc/passwd /etc/passwd && ln -sf /srv/php/etc/group /etc/group
COPY docker/php/fpm.conf /usr/local/etc/php-fpm.d/alveare.conf
COPY docker/php/php.ini /usr/local/etc/php/conf.d/alveare.ini
COPY --from=agent /src/target/release/php-agent /usr/local/bin/php-agent

# Override globale del pannello (conf.d/zz-custom.ini), dopo le impostazioni dell'immagine.
ENV PHP_INI_SCAN_DIR=/usr/local/etc/php/conf.d:/srv/php/conf.d

STOPSIGNAL SIGTERM
CMD ["php-agent"]
