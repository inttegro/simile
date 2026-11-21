# Simile 🎭

**Test against the service you expect.**

*Videri quam esse.*

Simile runs provider-authored service emulations on a developer's machine or in CI. It validates
requests, produces contract-backed responses, maintains local state, and exercises declared
asynchronous work, webhooks, scenarios, and delivery faults.

Service providers publish the public contract and optional behavior. Developers run the resulting
service without depending on a hosted test environment.

[Documentation](https://os.inttegro.dev/simile) ·
[Quickstart](https://os.inttegro.dev/simile/quickstart) ·
[Downloads](https://os.inttegro.dev/simile/downloads) ·
[Why Simile](https://os.inttegro.dev/simile/why-simile)

## Install

macOS and Linux:

```sh
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/inttegro/simile/HEAD/install.sh)"
```

Homebrew:

```sh
brew install inttegro/tap/simile
```

See [Downloads](https://os.inttegro.dev/simile/downloads) for Windows, pinned versions, and
checksum verification.

## Try an operation

Run any operation directly from a contract:

```sh
simile run openapi.yaml --operation-id createOrder
```

Simile synthesizes a valid request when none is supplied and prints the response as JSON. Supply a
complete request when the values matter:

```sh
simile run openapi.yaml \
  --operation-id createOrder \
  --request '{"body":{"product":"custom_jingle","quantity":2}}'
```

Continue with the [Quickstart](https://os.inttegro.dev/simile/quickstart) to register and run a
long-lived local service from a conventional `Simfile`.

## What Simile models

- HTTP operations described by OpenAPI 3.0–3.2, Smithy 2.0, or semantic TypeScript declarations.
- Optional HCL behavior for outcomes, state changes, asynchronous work, webhooks, and scenarios.
- Deterministic local backends for databases, queues, caches, mail, object storage, events,
  schedules, and outbound HTTP.
- Reproducible request and response generation, idempotent replay, causal state, and environmental
  fault injection.

Read [Define a service](https://os.inttegro.dev/simile/define-a-service) to author an emulation, or
[Architecture](https://os.inttegro.dev/simile/architecture) to understand the compiler and runtime.

## Status

Simile is pre-1.0 and under active development. Contracts, runtime behavior, and command-line
interfaces may evolve between minor releases. The
[current boundaries](https://os.inttegro.dev/simile/current-boundaries) page states what Simile can
prove and emulate today.

See the [changelog](CHANGELOG.md), [contribution guide](CONTRIBUTING.md), and
[service charter](CHARTER.md) for release history, development guidance, and product principles.

## License

[MIT](LICENSE)
