---
title: GitHub Actions
description: Run integration tests against Simile in a GitHub Actions job.
---

The workflow below installs a pinned Simile release, compiles the repository's `Simfile`, provisions
a CI caller profile, and supervises the test command.

```yaml
name: integration

on:
  pull_request:
  push:
    branches: [main]

permissions:
  contents: read

jobs:
  test:
    runs-on: ubuntu-latest
    env:
      SIMILE_HOME: ${{ runner.temp }}/simile

    steps:
      - uses: actions/checkout@v4

      - name: Install Simile
        run: |
          curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
            https://github.com/inttegro/simile/releases/download/v0.5.1/simile-cli-installer.sh \
            | sh

      - name: Install application dependencies
        run: npm ci

      - name: Register the provider
        run: simile register

      - name: Set up the CI caller
        run: |
          simile setup inttegro \
            --profile ci \
            --set name="GitHub Actions"

      - name: Run integration tests
        run: simile test inttegro --profile ci -- npm run test:integration
```

The test process receives `SIMILE_BASE_URL` and the values issued by the profile. A failed test exits
the step with the same nonzero status. Simile shuts the service down after the command completes.

## Services without setup

When the service has no caller provisioning, the registration and setup steps can be omitted:

```yaml
- name: Run integration tests
  run: simile test -- npm run test:integration
```

This form loads `./Simfile` and compiles it for the job.

## Test named scenarios

A matrix can exercise stable public outcomes independently:

```yaml
strategy:
  fail-fast: false
  matrix:
    scenario: [payment_declined, payment_requires_action]

steps:
  - uses: actions/checkout@v4
  # Install Simile and application dependencies first.
  - name: Test ${{ matrix.scenario }}
    run: |
      simile test inttegro \
        --profile ci \
        --scenario "${{ matrix.scenario }}" \
        -- npm run test:scenario -- "${{ matrix.scenario }}"
```

Each matrix job receives a separate `${{ runner.temp }}` directory and therefore a separate Simile
registry and database.

## Handle credentials and artifacts

Pass collected setup inputs through GitHub secrets when the service requires them:

```yaml
- name: Set up the CI caller
  env:
    WEBHOOK_URL: ${{ secrets.TEST_WEBHOOK_URL }}
  run: |
    simile setup inttegro \
      --profile ci \
      --set name="GitHub Actions" \
      --set webhook_url="$WEBHOOK_URL"
```

Simile conceals issued values in command output. Keep the job's `SIMILE_HOME` out of caches and
uploaded artifacts because it contains the durable profile and its private credential export.
