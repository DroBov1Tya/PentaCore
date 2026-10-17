# syntax=docker/dockerfile:1

# Build stage; none of it ends up in the image. perl and make build the
# bundled OpenSSL that SQLCipher uses. Debian 13 because the prebuilt ONNX
# runtime needs its newer C and C++ libraries.
FROM rust:1.95-slim-trixie AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends perl make pkg-config g++ \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked && mkdir /data

# Runtime: distroless. Only glibc, libstdc++ and CA certificates beside the
# binary; no shell, no package manager. Alpine is not an option: the ONNX
# runtime is built against glibc.
FROM gcr.io/distroless/cc-debian13:nonroot
COPY --from=build /src/target/release/pentacore /usr/local/bin/pentacore
COPY --from=build --chown=nonroot:nonroot /data /data
ENV PENTACORE_HOME=/data
VOLUME /data
# MCP over stdio: run with `docker run -i`.
ENTRYPOINT ["/usr/local/bin/pentacore"]
