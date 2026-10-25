# syntax=docker/dockerfile:1
#
# docker build -t nx-remote-cache .

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

# Just the binary, under its architecture:
#   docker buildx build --target binary --output type=local,dest=out .   → out/amd64/nx-remote-cache
# Releases replace this stage with the binaries they publish (--build-context binary=<dir holding
# amd64/ and arm64/>), so the image ships the very same file as the release assets.
FROM scratch AS binary
ARG TARGETARCH
COPY --from=build /nx-remote-cache /${TARGETARCH}/nx-remote-cache

# glibc + CA certificates, no shell, runs as uid 65532.
FROM gcr.io/distroless/cc-debian12:nonroot
ARG TARGETARCH
COPY --from=binary /${TARGETARCH}/nx-remote-cache /usr/local/bin/nx-remote-cache
# Mount a config.toml here, or pass the whole config in NX_CACHE_CONFIG_TOML.
WORKDIR /etc/nx-remote-cache
# The native default is loopback; in a container the port has to be reachable from outside it.
# Publish it on the host's loopback (`-p 127.0.0.1:3000:3000`).
ENV NX_CACHE__SERVER__LISTEN=0.0.0.0:3000
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/nx-remote-cache"]
CMD ["serve"]
