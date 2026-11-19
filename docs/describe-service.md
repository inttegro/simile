---
title: Describe a service
description: Read the public meaning of a registered or transient service configuration.
---

`simile describe service` explains the semantic objects in the same compiled artifact that Simile
runs. It is the human-facing companion to the technical `simile inspect` command.

```console
$ simile describe service inttegro
```

The default view shows the service description, listener, contract version, scenario catalogue,
faults and profiles, abstract resources, and asynchronous operations. Catalog faults carry built-in
descriptions automatically.

Add `--verbose` to include every operation and its compiled outcomes:

```console
$ simile describe service inttegro --verbose
```

Use stable JSON when another tool needs the description:

```console
$ simile --output json describe service inttegro
```

## Describe without registering

Pass a configuration path or URL to compile and describe it without changing the local registry:

```console
$ simile describe service inttegro --config ./inttegro.simile.hcl
$ simile describe service inttegro --config https://example.com/inttegro.simile.hcl
```

The service label in the HCL must still match `inttegro`. Relative OpenAPI and behavior paths work
for a local configuration. A remote configuration has no local base directory, so it must use an
OpenAPI URL and inline behavior declarations.

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
- `behavior` and `outcome`
- `error_set` and its outcomes
- `protocol_error`
- `scenario`
- `resource` and `async`
- custom `fault` and `fault_profile`

Structural blocks such as `listen`, `openapi`, `response`, `when`, `select`, `inject`,
`delay`, and `disconnect` do not accept descriptions. Their meaning comes from their parent object.

## Describe versus inspect

Use `describe` to answer “what can this service do, and what named test controls exist?” Use
`inspect` to answer “what is in this compiled file, which format version produced it, and what is
its digest?” Keeping these commands separate prevents compiler diagnostics from becoming the
service's public documentation model.
