# Immagine PHP condivisa da tutti i siti: un pool FPM per sito, generato dal pannello.
FROM rust:1-slim-trixie AS agent
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release -p php-agent

FROM php:8.5-fpm
ARG WP_CLI_VERSION=2.12.0

COPY --from=mlocati/php-extension-installer:2 /usr/bin/install-php-extensions /usr/local/bin/
RUN install-php-extensions gd intl zip mysqli exif redis pcntl \
 && apt-get update && apt-get install -y --no-install-recommends mariadb-client less \
 && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /usr/local/bin/wp \
      "https://github.com/wp-cli/wp-cli/releases/download/v${WP_CLI_VERSION}/wp-cli-${WP_CLI_VERSION}.phar" \
 && chmod 0755 /usr/local/bin/wp

# Via i pool di default (www, porta 9000): esistono solo i pool dei siti.
RUN rm -f /usr/local/etc/php-fpm.d/*.conf /usr/local/etc/php-fpm.d/*.default \
 && mkdir -p /srv/php/pools /srv/php/staging /run/php /run/alveare
# Utenti dei siti generati dal pannello in /srv/php/etc: con un link (e non un bind mount
# del singolo file) la sostituzione atomica del pannello si vede subito.
RUN ln -sf /srv/php/etc/passwd /etc/passwd && ln -sf /srv/php/etc/group /etc/group
COPY docker/php/fpm.conf /usr/local/etc/php-fpm.d/alveare.conf
COPY docker/php/php.ini /usr/local/etc/php/conf.d/alveare.ini
COPY --from=agent /src/target/release/php-agent /usr/local/bin/php-agent

STOPSIGNAL SIGTERM
CMD ["php-agent"]
