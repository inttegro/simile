---
title: Configuration
description: Compose one runnable service from its contract, listener, and optional behavior modules.
---

A service configuration is the composition root for one locally runnable emulator. It names the
service, selects one contract source, defines its listener, and optionally imports or contains HCL
behavior declarations. `simile register` validates and compiles the complete configuration before
installing it, while `simile start --config` retains the direct, unregistered path.

The configuration does **not** describe endpoints. The selected contract already owns them. Think
of this file as the place where a deployable local service is assembled from independently
reviewable sources.
If an agent is preparing both the contract and configuration, start with the
[service authoring prompt](/service-authoring-prompt). For the normative grammar rather than a
task-oriented guide, use the [Simile HCL language specification](/hcl-language-specification).

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

webhooks {
  ordering                 = "best_effort"
  out_of_order_probability = 0.10
  reorder_window           = "5s"
  concurrency              = 8

  signing {
    secret = "whsec_simile_local"
  }
}

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

The optional `webhooks` block controls service-wide delivery infrastructure. Its default models
bounded, reproducible out-of-order delivery instead of promising global ordering that real webhook
systems rarely provide. The nested signing block uses a dedicated local fixture; never place a
production webhook secret in a Simile definition. See [Webhook delivery](/webhook-delivery).

The document must contain exactly one `service` block. Its label is the service name passed to the
CLI:

```console
$ simile register inttegro --config ./inttegro.simile.hcl
$ simile register inttegro --config https://example.com/inttegro.simile.hcl
$ simile update inttegro
$ simile update inttegro --config ./replacement.simile.hcl
$ simile update inttegro --config https://example.com/inttegro.simile.hcl
$ simile setup inttegro
$ simile setup inttegro --profile checkout
$ simile profiles inttegro
$ simile profile inttegro checkout
$ simile use inttegro --profile checkout
$ simile remove inttegro --confirm inttegro
$ simile start inttegro
$ simile start inttegro --config ./inttegro.simile.hcl
$ simile start inttegro --config ./inttegro.simile.hcl --port 8484
$ simile start inttegro --scenario lost_after_commit
$ simile start inttegro --fault-profile rough_network
$ simile describe service inttegro
$ simile describe service inttegro --verbose
```

`--port` has invocation precedence over `listen.port`; it does not edit the configuration.
`simile describe service` reads the same compiled artifact that `start` runs. Pass `--config` to
describe a file or URL without registering it. See [Describe a service](/describe-service).

```mermaid
flowchart LR
  Config[Service HCL] --> Listener[Listener address]
  Config --> Contract[One contract source]
  Config --> Behaviors[Zero or more HCL modules]
  Contract --> Compile[Compile and validate]
  Behaviors --> Compile
  Compile --> Service[Runnable artifact]
```

## Local registry

The default registry root is `$HOME/.simile`; `SIMILE_HOME` overrides it for CI and tests. A
registration is an immutable runtime snapshot:

```text
~/.simile/
├── layout-version
├── services/
│   └── inttegro/
│       ├── current
│       ├── revisions/
│       │   └── REVISION/
│       │       ├── manifest.json
│       │       ├── config.hcl
│       │       └── service.similec
├── state/
│   ├── services/
│   │   └── inttegro.redb
│   └── stack/
│       └── KIND/
│           └── RESOURCE_FINGERPRINT.redb
├── profiles/
│   └── inttegro/
│       └── checkout.env
├── locks/
└── tmp/
```

The copied HCL preserves the registered source for inspection. The versioned JSON manifest records
listener and provenance metadata. The deterministic `.similec` artifact is the runtime authority.
Consequently, starting a registered service neither depends on the original config path nor
refetches a URL-based contract source. `simile update NAME` recompiles the stored HCL after reloading
the contract and imported behavior sources it names. `simile update NAME --config URL_OR_FILE`
first replaces that HCL source. The fetched HCL is kept in the installed revision; it is not saved
as a separate download. Compilation completes before `current` advances, so a failed update leaves
the previous revision active. Existing revisions remain available for recovery, while `current` is
the only mutable pointer. `register --force` remains an explicit replacement path, but normal
refreshes should use `update`. `locks/` is reserved for cross-process coordination as registry
operations expand; `tmp/` contains only same-filesystem staging directories used before an atomic
revision install. Stack component processes persist automatically under the root `state/stack/`
directory. The service supervisor checkpoints its complete Stack under `state/services/`; that
state remains stable across immutable registry revisions and is identity-checked against the
compiled artifact when the service starts.

## Consumer setup

An optional top-level `principal_setup` block describes how Simile provisions one declared
`principal`. It names a contract operation, maps collected values into that operation's request,
and declares which response or generated values Simile issues. Each run creates a durable named
profile of that principal.

Setup stays in the service configuration because it is a service-wide entry point. Imported
behavior modules cannot declare it. The compiler verifies its operation and security-scheme
references. Secrets are withheld from terminal and JSON output and written to a private profile
environment file. See [Set up a service](/service-setup) and
[Service profiles](/service-profiles).

## Listener

`listen.port` is required and must be an integer from 1 through 65535. `listen.host` is optional
and defaults to the loopback address `127.0.0.1`. The host must currently be an IP address rather
than a DNS name. Binding a public interface is therefore a deliberate configuration choice, not a
CLI default.

## Contract source

Choose exactly one of `openapi`, `smithy`, or `typescript`. The selected block must contain exactly
one source:

```hcl
openapi { path = "./commerce.yml" }
```

or:

```hcl
openapi { url = "https://example.com/commerce.yml" }
```

```hcl
smithy { path = "./commerce.smithy" }
```

```hcl
typescript { path = "./typescript/commerce.d.ts" }
```

A relative path is resolved from the directory containing the service configuration. A URL is read
into memory for compilation and is not saved to disk. A split TypeScript package requires a local
entrypoint or directory; a TypeScript URL must be a self-contained declaration document.

Both `simile register --config` and `simile update --config` can load the service configuration
itself from an HTTP or HTTPS URL. Because that document has no local directory, it must use a
contract URL and inline behavior declarations. Relative contract and imported behavior paths in a
remote configuration are rejected rather than resolved against the process working directory.

## Behavior modules

`behaviors` is an optional list of HCL module paths. Relative paths resolve from the configuration
directory. The service document may also contain `backend`, `async`, `effect`, `behavior`,
`scenario`, `faults`, `fault`, `fault_profile`, `principal`, `profile`, `webhook`, and `webhooks` blocks after the
`service` block:

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

webhook "order.created" {
  description = "Sent after an order has committed."
}

principal "merchant_application" {
  description = "A local merchant integration with order write access."
  credential "merchantOAuth" {
    grants = ["orders:write"]
  }
}

profile "merchant_app" {
  principal = principal.merchant_application
  credential "merchantOAuth" { value = "sim_test_merchant" }
}

behavior "createOrder" {
  response { status = 201 }
}

faults {
  use "http.slow" {}
  use "http.response_lost" {}
}
```

Backends are the portable supporting systems of the emulated service. They name capabilities such
as `db`, `cache`, `queue`, `mail`, `object_store`, `event_bus`, `scheduler`, or `http`. Production
vendor details stay outside HCL. Async declarations name post-response work. Effects attach synchronous actions
and named async units to operations; the compiler turns each action into a typed Stack command.
Webhook contracts normally come from OpenAPI callbacks or top-level webhooks. A singular
`webhook` block fills a public-contract gap, while `effect.emit` attaches one or more outbound
occurrences to committed operations. The plural `webhooks` block configures delivery
infrastructure once for the service.

Imported and inline declarations enter the same semantic compiler. Duplicate declarations,
unknown contract operation IDs, undeclared backends, and contradictory response selections fail
startup before the listener binds.

The plural `faults` block imports complete environmental faults from Simile's built-in catalog.
Catalog defaults can be overridden locally without repeating their phase and action mechanics. See
[Fault injection](/fault-injection) for exact imports, whole-catalog and namespace wildcards,
exclusions, available presets, and composition rules.

## Current HTTP boundary

The server compiles contract routes into a route table and delegates matched operations to the
protocol-neutral runtime. It currently provides:

- literal and `{parameter}` path-template matching, with literal routes taking precedence;
- OpenAPI-aware decoding and compiled-schema validation for path, query, header, and cookie parameters;
- `404 Not Found` and `405 Method Not Allowed` responses at the transport boundary;
- an 8 MiB request-body limit;
- JSON, form URL-encoded, multipart form-data, and text request-body decoding through the runtime;
- OpenAPI security enforcement through principal contracts, profiles, and declared grants;
- generated contract response status, media type, and body;
- named contract and HCL outcomes, including an optional process-wide `--scenario NAME`;
- deterministic scenario fault injection and an optional bounded stochastic
  `--fault-profile NAME`;
- schema-checked planning of effect-emitted webhook occurrences, including supported callback
  destination expressions, explicit destinations, delays, and hard dependencies;
- service-declared protocol-error mappings for malformed or contract-invalid requests, with a
  Simile diagnostic only when the operation has no suitable error response;
- process-local replay for `Idempotency-Key` and for the full URI of GET requests; and
- transactional synchronous effects, durable Stack checkpoints, and a crash-safe deferred-work outbox;
- leased async and webhook delivery with retries and dead-letter storage;
- optional HMAC-SHA256 webhook signatures; and
- one-line access logs containing method, path, status, TCP peer IP, user agent, request content type
  and size, response content type and size, and elapsed microseconds; and
- graceful shutdown on Ctrl-C.

Access logs are emitted to stderr at `info` level by default for `simile start`. Pass `--quiet` to
disable them, or use `SIMILE_LOG` to select a tracing filter explicitly. The origin is the accepted
socket's peer IP; Simile does not trust forwarding headers. Request and response bodies, query
strings, credentials, idempotency keys, and header values other than `User-Agent` are never written
to the access log.

Mutual TLS authentication still awaits a TLS-aware listener. Ambiguous state transitions and
provider-specific error decisions require authored HCL. Simile reports those boundaries through
`simile audit` so a runnable service never silently acquires authority its sources did not provide.

Continue with [Lifecycle](/service-lifecycle) to see how this file becomes a registered
revision, [Fault injection](/fault-injection) for environmental failure controls, or
[Contract sources and HCL](/contract-sources-and-hcl) to decide what belongs in each source.
