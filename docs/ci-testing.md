---
title: CI testing
description: Give every CI job an isolated provider emulator and propagate test failures correctly.
---

Use `simile test` as the parent process for an integration test command. Simile binds the emulator,
exports its address and selected profile, and exits with the test command's status. CI receives one
result without background-process scripts or readiness polling.

## A portable job

This shell sequence works in most CI systems:

```bash
export SIMILE_HOME="$PWD/.simile-ci"

simile register
simile setup inttegro \
  --profile ci \
  --set name="CI integration"

simile test inttegro --profile ci -- ./scripts/integration-test
```

`register` compiles the Simfile before tests begin. `setup` provisions the caller identity declared
by the service. The test process reads `SIMILE_BASE_URL` and any issued values such as `API_KEY`.

If the service has no principal setup, the shorter flow is enough:

```bash
export SIMILE_HOME="$PWD/.simile-ci"
simile test -- ./scripts/integration-test
```

## Isolate jobs and workers

Give each independently running job its own Simile home:

```bash
export SIMILE_HOME="$RUNNER_TEMP/simile-$CI_JOB_ID"
```

An isolated home provides:

- a fresh registry and service database;
- independent idempotency and causal history;
- no port collision, because `simile test` uses an available port; and
- predictable cleanup when the CI workspace is discarded.

Parallel workers inside one job should also use distinct directories. Reusing one directory is
useful only when later steps intentionally continue the same emulated service state.

## Make failures reproducible

Use named scenarios for documented business outcomes:

```bash
simile test inttegro \
  --scenario payment_declined \
  -- ./scripts/integration-test payment-declined
```

Use a fixed seed declared by the selected fault profile when a stochastic transport run needs exact
reproduction. Preserve application test reports and Simile reproduction bundles as CI artifacts.
Avoid uploading `SIMILE_HOME`: it may contain issued credentials and collected setup values.

## Pin the tool and service definition

Pin the Simile release used by the job. Keep the Simfile, HCL modules, and local contract
sources in version control, or point the Simfile at an immutable provider revision. Registration
then compiles the same public service model on every run.

For GitHub-hosted jobs, continue with [GitHub Actions](/github-actions). For containerized runners,
see [Docker](/docker).
