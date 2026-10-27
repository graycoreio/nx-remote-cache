# syntax=docker/dockerfile:1

# Compile the release binary from source inside a full Rust toolchain image.
FROM rust:1.99-bookworm AS build

# aws-lc-sys (rustls' crypto provider) builds C code.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake clang \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && cp target/release/nx-remote-cache /nx-remote-cache

# Place the binary under its target architecture so release builds can swap in prebuilt binaries.
FROM scratch AS binary

# Expose BuildKit's target architecture (amd64, arm64) to this stage.
ARG TARGETARCH

COPY --from=build /nx-remote-cache /${TARGETARCH}/nx-remote-cache

# glibc + CA certificates, no shell, runs as uid 65532.
FROM gcr.io/distroless/cc-debian13:nonroot

LABEL org.opencontainers.image.authors="Peter Lauck <peter.lauck@graycore.io>"

# Expose BuildKit's target architecture (amd64, arm64) to this stage.
ARG TARGETARCH

COPY --from=binary /${TARGETARCH}/nx-remote-cache /usr/local/bin/nx-remote-cache

# Mount a config.toml here, or pass the whole config in NX_CACHE_CONFIG_TOML.
WORKDIR /etc/nx-remote-cache

# The native default is loopback; in a container the port has to be reachable from outside it.
ENV NX_CACHE_DEFAULT_LISTEN=0.0.0.0:3000
EXPOSE 3000

ENTRYPOINT ["/usr/local/bin/nx-remote-cache"]
CMD ["serve"]
