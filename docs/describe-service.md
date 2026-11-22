---
title: Describe a service
description: Read the public meaning of a registered or transient service configuration.
---

`simile describe` explains the semantic objects compiled from the current `Simfile`. It is the
human-facing companion to the technical `simile inspect` command.

```console
$ simile describe
```

To inspect an installed service from another directory, name its registry entry:

```console
$ simile describe service inttegro
```

The default view is a compact inventory. It groups runtime identity, contract facts, and service
model counts so that a large service still fits comfortably in one terminal screen:

```text
orders 1.0.0
An emulator for order integration tests.

Runtime
  Listen               http://127.0.0.1:4242
  Source               registry
  Issue reports        https://reports.example.com/v1/reports

Contract
  Format               OpenAPI 3.1.2
  Operations           37
  Webhook contracts    2

Service model
  Principals           1
  Service profiles     2
  Setup workflows      1
  Scenarios            6
  Faults               4
  Fault profiles       1
  Protocol errors      7
  Backends             3
  Async operations     2
  Error/outcome docs   12
```

Credential values are omitted from every human-readable view.

`Issue reports` appears only when the service declares a reporting destination. Consent and
contact remain profile-owned and appear through `simile reporting SERVICE`, with the address
masked.

Add `--verbose` to expand the inventory into named, indented entries. This includes every
operation, its compiled outcomes, its authentication surface, whether its behavior came from the
contract or provider-authored HCL, service profiles, setup workflows, webhook contracts,
scenarios, faults, backends, asynchronous operations, and public error or outcome descriptions.
Catalog faults carry their built-in descriptions automatically:

```console
$ simile describe service inttegro --verbose
```

Use stable JSON when another tool needs to parse the description. Human output may evolve to make
terminal reading clearer; JSON field names and structure form the automation interface:

```console
$ simile --output json describe service inttegro
```

## Describe without registering

The default `simile describe` command compiles `./Simfile` without changing the local registry.
Pass a configuration path or URL to select another definition:

```console
$ simile describe service --config ./inttegro.simile.hcl
$ simile describe service --config https://example.com/inttegro.simile.hcl
```

Supplying a service name verifies that the HCL label matches it. Relative contract and behavior
paths work for a local configuration. A remote configuration has no local base directory, so it
must use a contract URL and inline behavior declarations.

## `description` metadata

Named semantic objects may carry a public human-readable description:

```hcl
scenario "payment_declines" {
  description = "Force the public declined-payment path for checkout tests."

  select "createOrder" {
    outcome = "payment_method_declined"
  }
}
```

Descriptions are retained in the compiled artifact. They never change request matching, outcome
selection, fault activation, or execution. Write them as public integration documentation: do not
include credentials, private thresholds, internal system names, or production decision trees.

The following named blocks accept `description`:

- `service`
- `principal_setup` and its `collect` and `issue` blocks
- `behavior` and `outcome`
- `error_set` and its outcomes
- `protocol_error`
- `scenario`
- `backend`, `principal`, `profile`, and `async`
- `effect` and provider-authored `webhook`
- custom `fault` and `fault_profile`

Structural blocks such as `listen`, `openapi`, `response`, `when`, `select`, `inject`,
`webhooks`, `delay`, and `disconnect` do not accept descriptions. Their meaning comes from their
parent object.

## Describe versus inspect

Use `describe` to answer “what can this service do, and what named test controls exist?” Use
`inspect` to answer “what is in this compiled file, which format version produced it, and what is
its digest?” Keeping these commands separate prevents compiler diagnostics from becoming the
service's public documentation model.
