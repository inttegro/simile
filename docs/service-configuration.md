---
title: Configuration
description: Compose one runnable service from its contract, listener, and optional behavior modules.
---

A `Simfile` is the composition root for one service. It gives the service a name, selects its
contract, configures its listener, and adds any provider-authored behavior. Simile compiles these
inputs into one validated runtime artifact.

Use the [Define a service](/define-a-service) guide for a guided build. This page is a compact
reference for the configuration itself. The [HCL language specification](/hcl-language-specification)
defines the complete grammar.

```hcl
service "inttegro" {
  description = "A local emulator for Inttegro Commerce integrations."

  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./commerce.yml"
  }

  behaviors = [
    "./orders.hcl",
    "./payments.hcl",
  ]
}
```

The document contains exactly one `service` block. Its label, `inttegro` above, becomes the service
name.

## Local workflow

Place the definition in `./Simfile`. Simile discovers that file in the current directory and infers
the service name:

```console
$ simile describe
$ simile register
$ simile start
```

Discovery checks the current directory only. A named service uses its installed registration, while
`--config` selects a particular local or remote definition:

```console
$ simile register inttegro --config ./inttegro.simile.hcl
$ simile register inttegro --config https://example.com/inttegro.simile.hcl
$ simile update inttegro
$ simile update inttegro --config ./replacement.simile.hcl
$ simile start inttegro
$ simile start inttegro --config ./inttegro.simile.hcl --port 8484
$ simile describe service inttegro --verbose
$ simile describe service --config ./inttegro.simile.hcl
```

`--port` takes precedence over `listen.port` for that invocation. The configuration file remains
unchanged. See [Lifecycle](/service-lifecycle) for registration and updates, and
[Describe a service](/describe-service) for inspection commands.

## Listener

`listen.port` is required and accepts an integer from 1 through 65535. `listen.host` defaults to the
loopback address `127.0.0.1` and currently accepts an IP address. Use an explicit non-loopback
address when other machines must reach the emulator.

## Contract source

Choose one contract format and one source:

```hcl
openapi { path = "./commerce.yml" }
```

```hcl
openapi { url = "https://example.com/commerce.yml" }
```

```hcl
smithy { path = "./commerce.smithy" }
```

```hcl
typescript { path = "./typescript/commerce.d.ts" }
```

Each contract block accepts either `path` or `url`. Multi-file Smithy models and TypeScript packages
use a local entrypoint or directory. A contract loaded from a URL must be self-contained. See
[Contract sources and HCL](/contract-sources-and-hcl) for format selection and source boundaries.

## Behavior and service declarations

`behaviors` imports HCL modules by path. The service file can also declare `async`, `backend`,
`behavior`, `effect`, `fault`, `fault_profile`, `faults`, `principal`, `profile`, `scenario`,
`webhook`, and `webhooks` blocks directly.

```hcl
backend "queue" "orders" {}

async "dispatch_order" {
  action "queue.orders" "publish" {
    body = response.order
  }
}

effect "commit_order" {
  operations = ["createOrder"]
  async      = ["dispatch_order"]
  emit = {
    "order.created" = response.order
  }
}

faults {
  use "http.slow" {}
  use "http.response_lost" {}
}
```

Imported and inline declarations pass through the same compiler. Compilation rejects duplicate
names, unknown operation IDs, undeclared backends, and contradictory response selections before the
listener binds.

Dedicated references cover [dependencies and effects](/dependencies-and-effects),
[durable work](/durable-work), [fault injection](/fault-injection), and
[Simile Stack](/simile-stack).

## Webhooks

A singular `webhook` block supplements webhook contracts absent from OpenAPI. `effect.emit` connects
committed operations to outbound webhook occurrences. A service-wide `webhooks` block controls
delivery, concurrency, reordering, and signing:

```hcl
webhooks {
  ordering                 = "best_effort"
  out_of_order_probability = 0.10
  reorder_window           = "5s"
  concurrency              = 8

  signing {
    secret = "whsec_simile_local"
  }
}

webhook "order.created" {
  description = "Sent after an order has committed."
}
```

Use a dedicated local signing fixture. Keep production webhook secrets outside Simile definitions.
See [Webhook delivery](/webhook-delivery) for delivery guarantees and ordering controls.

## Principals and setup

A `principal` describes an identity accepted by the service. A matching top-level
`principal_setup` tells Simile how to provision that identity and save it as a named profile:

```hcl
principal "application" {
  credential "secretKey" {}
}

principal_setup "application" {
  description = "Create an application identity for this integration."
  operation   = "createApplication"

  collect "name" {
    required = true
    to       = request.name
  }

  issue "secret_key" {
    from       = response.app.secret_key
    credential = "secretKey"
    sensitive  = true
  }
}
```

The compiler checks operation and security-scheme references. Simile stores sensitive issued values
in the profile's private environment file and omits them from terminal and JSON output. Run
`simile setup NAME`, then inspect or select profiles with `simile profiles`, `simile profile`, and
`simile use`. See [Principals](/principals), [Set up a service](/service-setup), and
[Service profiles](/service-profiles).

`principal_setup` belongs in the service configuration because it provisions the service-wide entry
point. Imported behavior modules cannot declare it.

## Source resolution

Relative contract and behavior paths resolve from the directory containing the configuration file.
This rule stays stable when the command runs from another working directory.

`--config` accepts a local path or an HTTP or HTTPS URL. A remote configuration uses contract URLs
and inline behavior declarations because it has no local base directory. Simile rejects relative
contract and behavior paths in remote configurations.

During registration, Simile reads remote sources into memory and stores the compiled service. A
later `simile update NAME` reloads the sources named by the stored HCL. Passing `--config` replaces
that HCL before compilation.

## Registry and state

Simile stores registrations under `$HOME/.simile`. Set `SIMILE_HOME` to choose another root for CI
or tests.

```text
~/.simile/
├── layout-version
├── services/NAME/
│   ├── current
│   └── revisions/REVISION/
│       ├── config.hcl
│       ├── manifest.json
│       └── service.similec
├── profiles/NAME/PROFILE.env
├── state/
│   ├── services/NAME.redb
│   └── stack/KIND/RESOURCE_FINGERPRINT.redb
├── locks/
└── tmp/
```

Each revision contains the source HCL, provenance manifest, and deterministic `.similec` runtime
artifact. A remote configuration becomes that revision's `config.hcl`. Registered startup reads the
artifact, so the original file or URL is unnecessary at runtime.

Updates compile a complete revision before advancing `current`. A compilation failure preserves the
active revision, and older revisions remain available for recovery. Use `simile update` for normal
refreshes; `register --force` explicitly replaces an existing registration. Runtime and Stack state
live outside immutable revisions under `state/`, allowing restarts and updates to resume persisted
work. Simile verifies persisted service state against the compiled artifact before restoration.
`locks/` supports registry coordination, and `tmp/` holds staging data during atomic installation.

For the runtime path from configuration to listener, continue with [HTTP server](/http-server).
Current protocol and fidelity limits are tracked in [Current boundaries](/current-boundaries).
