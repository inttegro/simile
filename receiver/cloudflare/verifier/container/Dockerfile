# syntax=docker/dockerfile:1

# Build on the host architecture so Apple-silicon development does not run GCC
# under QEMU, while still producing the linux/amd64 binary Cloudflare requires.
FROM --platform=$BUILDPLATFORM rust:1.85-bookworm AS builder
WORKDIR /src
# Install the repository's exact toolchain before COPY makes rustup select it;
# otherwise the cross target lands in the image's newer default toolchain.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        gcc-x86-64-linux-gnu libc6-dev-amd64-cross \
    && rm -rf /var/lib/apt/lists/* \
    && rustup toolchain install 1.85.0 --profile minimal \
        --component clippy --component rustfmt \
        --target x86_64-unknown-linux-gnu
COPY . .
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
ENV CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release --target x86_64-unknown-linux-gnu --bin simile \
    && cp /src/target/x86_64-unknown-linux-gnu/release/simile /tmp/simile

FROM node:24-bookworm-slim AS runtime
RUN useradd --create-home --user-group --uid 10001 verifier
COPY --from=builder /tmp/simile /usr/local/bin/simile
COPY receiver/cloudflare/verifier/container/server.ts /app/server.ts
USER verifier
WORKDIR /app
ENV PORT=8080
ENV SIMILE_HOME=/tmp/simile
EXPOSE 8080
ENTRYPOINT ["node", "/app/server.ts"]
