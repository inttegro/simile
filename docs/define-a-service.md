---
title: Define a service
description: Turn a provider's public contract into a validated, runnable Simile service.
---

This guide takes one service from public evidence to a running local emulator. At the end, you will
have a contract, a small Simile configuration, a compiled fidelity report, and a registered service
that applications can call over HTTP.

<Tip>
  If you want a disposable five-minute example, begin with the [Quickstart](/quickstart). Use this
  guide when you are defining a service that other people will depend on.
</Tip>

## The authoring path

```mermaid
flowchart LR
  Evidence[Public service evidence] --> Contract[One complete HTTP contract]
  Contract --> Definition[Service composition and behavior]
  Definition --> Proof[Compile, exercise, describe, and audit]
  Proof --> Runtime[Registered local HTTP service]
```

Keep these files together while authoring:

```text
orders/
├── orders.yaml
├── orders.simile.hcl
├── behavior/
│   └── orders.hcl
└── requests/
    └── create-order.json
```

The contract carries the complete HTTP surface. The service configuration selects that contract,
sets the listener, and composes optional behavior modules. The request fixture gives you one known
operation to exercise throughout the authoring loop.

## 1. Establish the public evidence

Write down the evidence available to an integration developer before choosing syntax:

- operations, methods, paths, and stable operation IDs;
- request parameters, bodies, content types, and validation rules;
- success and error responses;
- authentication schemes and operation permissions;
- relationships within a payload and from request to response;
- state changes, delayed work, and emitted webhooks;
- deterministic test outcomes and environmental faults; and
- examples that the provider promises to keep valid.

Use published API documentation, existing contracts, SDK types, integration tests, fixtures, and
public error catalogues. Resolve contradictions while the definition is still small. Record an
assumption explicitly when the public evidence cannot settle it.

The selected contract should remain useful in ordinary API tooling. HCL covers additional
observable behavior and local composition.

## 2. Choose one contract source

Choose the source that the provider can maintain as the complete public HTTP contract:

| Source | Good fit |
| --- | --- |
| [OpenAPI](/openapi-and-hcl) | Existing HTTP specifications and broad tooling compatibility |
| [Smithy](/smithy-contracts) | Typed service models, traits, mixins, and model evolution |
| [Semantic TypeScript](/typescript-contracts) | Declaration-first ecosystems and type-level payload relationships |

This guide uses OpenAPI and stores the contract in `orders.yaml`. The same workflow applies to a
Smithy or semantic TypeScript source.

Before adding HCL, prove that the contract compiles:

```bash
simile compile ./orders.yaml --artifact ./orders.similec
simile inspect ./orders.similec
```

Compilation should fail while a reference is unresolved, an operation is ambiguous, or a declared
relationship cannot be proved. Fix the contract at this boundary so later behavior can rely on one
coherent operation model.

## 3. Exercise one baseline operation

Choose an operation that represents the service's main path. Create `requests/create-order.json`:

```json
{
  "headers": {
    "Content-Type": "application/json"
  },
  "body": {
    "product": "licensed_beat",
    "quantity": 2
  }
}
```

Run the operation directly from the contract:

```bash
simile run ./orders.yaml \
  --operation-id createOrder \
  --request ./requests/create-order.json \
  --seed 7 \
  --verbose
```

Confirm that Simile selects the intended operation and media type, validates the request, chooses an
appropriate status, and emits a schema-valid response. A fixed seed makes the authoring run
repeatable while the contract changes.

Add public soundness declarations when fields depend on one another. Use
[`x-simile-constraints`](/x-simile-constraints) for relationships inside one payload and
[`x-simile-from-request`](/request-response-relations) for request-to-response construction.

## 4. Compose the runnable service

Create `orders.simile.hcl`:

```hcl
service "orders" {
  description = "A local emulator for the public Orders API."

  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./orders.yaml"
  }

  behaviors = [
    "./behavior/orders.hcl",
  ]
}
```

The service label becomes the CLI name. Relative paths resolve from the configuration file's
directory. Keep the listener on loopback unless another machine must reach it.

For Smithy or TypeScript, replace the `openapi` block with one `smithy` or `typescript` block. The
[Configuration reference](/service-configuration) covers local paths, URLs, listener precedence,
and remote configurations.

## 5. Add the smallest useful behavior layer

Begin `behavior/orders.hcl` with only the public behavior the contract leaves unresolved. For a
stateful resource API, declaring one database can enable conservative create, read, list, and
delete inference:

```hcl
backend "db" "primary" {
  description = "Durable order records exposed by the public API."
}
```

Simile can infer an ordinary write when a successful `POST`, `PUT`, or `PATCH` response contains
one identifiable entity and the service declares one database. Add an explicit
[`effect`](/dependencies-and-effects) when the operation uses a custom identity, writes several
entities, or has another provider-defined consequence.

Extend the behavior layer only when the service requires it:

| Public behavior | Declaration |
| --- | --- |
| Exact success or error outcome | `behavior`, `outcome`, or `error_set` |
| Durable state change | `backend` and `effect` |
| Work after the response commits | `async` attached by an `effect` |
| Outbound event delivery | OpenAPI callback/webhook plus `effect.emit` |
| Named caller kind and permissions | `principal` |
| Caller provisioning | `principal_setup` |
| Deterministic exceptional path | `scenario` |
| Latency, disconnect, or protocol disruption | `faults` and `fault_profile` |

Every declaration should describe behavior visible to an integrator. Opaque scenario names cover
public outcomes whose production decision rules remain private.

## 6. Prove the assembled definition

Describe the transient configuration before changing the local registry:

```bash
simile describe service orders --config ./orders.simile.hcl
simile --verbose describe service orders --config ./orders.simile.hcl
```

The description should read like a public service catalogue: listener, operations, principals,
setup workflows, outcomes, backends, faults, and webhooks. Improve vague names and descriptions
before continuing.

Compile the contract with the same behavior module and run the fidelity audit:

```bash
simile compile ./orders.yaml \
  --behavior ./behavior/orders.hcl \
  --artifact ./orders.similec

simile audit ./orders.similec
simile audit ./orders.similec --strict
```

The audit identifies behavior that remains schema-derived, convention-inferred, unsupported, or
underspecified. Resolve every error. Decide which warnings the first release may carry, then use
`--strict` as the acceptance gate for the chosen baseline.

Run representative success and error operations after each material change. Capture important
cases as [reproduction bundles](/reproduction-bundles) and group them into a
[conformance suite](/conformance-suites).

## 7. Register, set up, and start

Install the validated definition in the local registry:

```bash
simile register orders --config ./orders.simile.hcl
```

If the service declares one or more `principal_setup` workflows, create the caller profile before
starting:

```bash
simile setup orders
simile profiles orders
```

Then start the long-running service:

```bash
simile start orders
```

Registration stores an immutable compiled revision under `~/.simile`. Starting by name loads that
revision and restores compatible durable service state.

## 8. Verify the HTTP boundary

Keep Simile running and call it from another terminal:

```bash
curl --request POST http://127.0.0.1:4242/orders \
  --header 'Content-Type: application/json' \
  --data '{"product":"licensed_beat","quantity":2}'
```

Verify the response status, headers, body, persisted state, asynchronous work, and webhook delivery
that this operation promises. Use the same client library and authentication flow that the real
integration uses whenever possible.

After changing the contract or configuration, compile and audit again, then install a new revision:

```bash
simile update orders
```

Restart the running process to load the newly current revision. See [Lifecycle](/service-lifecycle)
for replacement configurations, removal, registry layout, and state behavior.

## Definition of done

A service definition is ready for other integrators when:

- the selected contract independently describes every supported HTTP operation;
- every operation has a stable `operationId`;
- representative valid and invalid requests select the expected public outcomes;
- important responses preserve request values and satisfy cross-field relationships;
- authentication, state changes, delayed work, and webhooks match published behavior;
- deterministic scenarios cover the exceptional paths integrators must test;
- environmental faults stay bounded and opt-in through declared profiles;
- `simile describe service` gives a useful public account of the service;
- the fidelity audit has no unexplained findings; and
- a real client can complete its main flow against `simile start`.

Continue with [Contract sources and HCL](/contract-sources-and-hcl) for the ownership model,
[Configuration](/service-configuration) for composition syntax, and
[Model behavior](/outcome-fidelity) for higher-fidelity outcomes and consequences.
