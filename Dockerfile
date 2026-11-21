# syntax=docker/dockerfile:1

FROM rust:1.85-bookworm AS builder
WORKDIR /src

COPY . .
RUN cargo build --locked --release --bin simile

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --user-group --uid 10001 simile \
    && install --directory --owner simile --group simile /var/lib/simile /workspace

COPY --from=builder /src/target/release/simile /usr/local/bin/simile

USER simile
WORKDIR /workspace
ENV SIMILE_HOME=/var/lib/simile

EXPOSE 8484
ENTRYPOINT ["simile"]
CMD ["--help"]
