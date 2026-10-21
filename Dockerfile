# syntax=docker/dockerfile:1
#
# docker build -t nx-remote-cache .
# docker build --build-arg FEATURES=mint -t nx-remote-cache:mint .

FROM rust:1.99-bookworm AS build
# aws-lc-sys (rustls' crypto provider) builds C code.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
ARG FEATURES=""
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked ${FEATURES:+--features "$FEATURES"} \
    && cp target/release/nx-remote-cache /nx-remote-cache

# glibc + CA certificates, no shell, runs as uid 65532.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /nx-remote-cache /usr/local/bin/nx-remote-cache
# Mount a config.toml here, or pass the whole config in NX_CACHE_CONFIG_TOML.
WORKDIR /etc/nx-remote-cache
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/nx-remote-cache"]
CMD ["serve"]
