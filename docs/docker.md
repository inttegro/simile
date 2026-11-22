---
title: Docker
description: Build the Simile image and run a service emulator in a container.
---

The repository includes a multi-stage `Dockerfile`. The runtime image contains the Simile binary,
CA certificates, a non-root user, `/workspace` for service sources, and `/var/lib/simile` as
`SIMILE_HOME`.

Docker helps when the application tests already run in containers or the CI platform standardizes
on container jobs. Native installation with `simile test` is the shortest path for ordinary local
and CI runs.

## Build the image

From the Simile repository:

```bash
docker build --tag simile:local .
```

The build uses Rust 1.85 and compiles the release binary with the locked dependency graph.

## Start a service from a Simfile

Mount the service project at `/workspace`, publish its listener, and override the configured host so
the port is reachable outside the container:

```bash
docker run --rm \
  --publish 8484:8484 \
  --volume "$PWD:/workspace:ro" \
  simile:local \
  start --host 0.0.0.0 --port 8484
```

The container loads `/workspace/Simfile`. Relative contract and module paths resolve from that
directory.

## Preserve service state

Mount a named volume at `/var/lib/simile` when registrations, profiles, resources, and replay state
should survive a container restart:

```bash
docker volume create simile-home

docker run --rm \
  --volume "$PWD:/workspace:ro" \
  --volume simile-home:/var/lib/simile \
  simile:local register

docker run --rm \
  --publish 8484:8484 \
  --volume "$PWD:/workspace:ro" \
  --volume simile-home:/var/lib/simile \
  simile:local \
  start inttegro --host 0.0.0.0 --port 8484
```

Use a disposable volume in CI for clean state. The volume can contain issued credentials, so keep
it private and delete it with the job.

## Connect an application container

Put the emulator and application test container on one Docker network:

```bash
docker network create integration

docker run --detach --rm \
  --name inttegro-emulator \
  --network integration \
  --volume "$PWD/provider:/workspace:ro" \
  simile:local \
  start --host 0.0.0.0 --port 8484

for attempt in $(seq 1 50); do
  docker logs inttegro-emulator 2>&1 | grep -q '^Started ' && break
  sleep 0.1
done
docker logs inttegro-emulator 2>&1 | grep -q '^Started '

docker run --rm \
  --network integration \
  --env SIMILE_BASE_URL=http://inttegro-emulator:8484 \
  application:test npm run test:integration

docker stop inttegro-emulator
docker network rm integration
```

One emulator process serves one Simfile. Launch one container per external provider when a test
environment needs several services. Container orchestration can assign a stable network name to
each emulator while every service keeps its own `SIMILE_HOME` volume.

For a single-process job with automatic readiness and cleanup, install Simile in the application
test image and run `simile test -- COMMAND` there. See [CI testing](/ci-testing) for that lifecycle.
