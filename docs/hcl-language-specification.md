---
title: Language specification
sidebarTitle: Spec
description: The complete syntax and semantics of the Simile HCL language.
---

This document defines the Simile HCL language implemented by Simile v0.2.0. It is the canonical
reference for service configuration documents and behavior modules. Task-oriented pages explain
when to use a feature; this specification defines what the feature means.

“HCL” in this reference means Simile's domain-specific language built on the
[HCL native syntax](https://github.com/hashicorp/hcl/blob/main/hclsyntax/spec.md). It is not
Terraform configuration. HCL supplies the concrete notation; Simile owns the permitted blocks,
attributes, types, defaults, name resolution, and compile-time rules.

## Language model

A Simile service has one contract source and an optional behavior source:

- **OpenAPI, Smithy, or semantic TypeScript** is the authority for HTTP routes, request schemas,
  response statuses, media types, and response schemas. Exactly one is selected.
- **Simile HCL** composes the runnable service and adds public behavior the contract cannot express,
  such as named outcomes, scenarios, abstract effects, and environmental faults.

HCL is an overlay. It cannot create an HTTP route, introduce an undeclared response status, replace
a schema, or contradict the contract. Every cross-source reference is resolved during compilation;
invalid references stop the service before it can run.

## Documents

Simile accepts two document forms.

### Service configuration

A service configuration contains exactly one `service` block and may contain behavior declarations
in the same file:

```hcl
service "inttegro" {
  listen { port = 4242 }
  openapi { path = "./commerce.yml" }
  behaviors = ["./orders.hcl"]
}

behavior "createOrder" {
  response { status = 201 }
}
```

The `behaviors` paths name additional behavior modules. Inline and imported declarations enter the
same compiler and namespace.

### Behavior module

A behavior module contains only behavior declarations. It has no `service` block:

```hcl
resource "queue" "orders" {}

async "dispatch_order" {
  uses = ["queue.orders"]
}
```

Behavior modules are accepted by `simile run --behavior` and by the `behaviors` attribute of a
local service configuration.

### Top-level forms

| Block | Service config | Behavior module | Purpose |
| --- | --- | --- | --- |
| [`service`](#service) | exactly one | not allowed | Compose the listener, contract source, and imported modules. |
| [`resource`](#resource) | optional | optional | Name an abstract dependency. |
| [`async`](#async) | optional | optional | Declare planned follow-up work. |
| [`behavior`](#behavior) | optional | optional | Refine one contract operation. |
| [`error_set`](#error_set) | optional | optional | Define reusable named outcomes. |
| [`apply_error_set`](#apply_error_set) | optional | optional | Attach an error set through contract tags. |
| [`protocol_error`](#protocol_error) | optional | optional | Map a runtime failure cause into service responses. |
| [`scenario`](#scenario) | optional | optional | Define deterministic outcome and fault selections. |
| [`faults`](#faults-and-use) | optional | optional | Import built-in environmental faults. |
| [`fault`](#fault) | optional | optional | Define a custom environmental fault. |
| [`fault_profile`](#fault_profile) | optional | optional | Define bounded stochastic fault activation. |

## Concrete syntax

Simile follows HCL's native block syntax:

```hcl
block_type "label" {
  attribute = expression

  nested_block "label" {
    attribute = expression
  }
}
```

The following rules apply throughout the language:

- block labels are quoted strings;
- attribute names and block types are case-sensitive;
- a top-level attribute is never valid;
- unknown blocks, nested blocks, and attributes are errors;
- comments use `#`, `//`, or `/* ... */` HCL syntax;
- strings, numbers, booleans, arrays, and objects use HCL literal syntax; and
- references to variables, Terraform resources, providers, functions, and dynamic blocks are not
  part of Simile HCL.

Most attributes require a literal of one exact type. The `body` and `equals` attributes accept any
expression that evaluates without external variables or functions and converts to a JSON value.

### Public descriptions

Named semantic blocks accept an optional `description` attribute: `service`, `resource`, `async`,
`behavior`, `outcome`, `error_set`, `protocol_error`, `scenario`, custom `fault`, and
`fault_profile`. Its value must be a non-empty string. Descriptions are presentation metadata
retained in the compiled artifact and displayed by `simile describe service`; they never influence
matching, selection, synthesis, or execution.

Structural blocks do not accept `description`: `listen`, `openapi`, `response`, `when`,
`apply_error_set`, `faults`, `use`, `select`, `inject`, `delay`, and `disconnect`. Their meaning
belongs to the named object that contains or references them. Descriptions are public integration
documentation and must not contain secrets, internal thresholds, private rules, or production
system names.

### Notation used below

In syntax summaries:

- `NAME`, `OPERATION_ID`, and similar uppercase words stand for quoted string labels;
- `[...]` in prose means optional syntax, not an HCL array;
- `...` means a form may repeat; and
- an attribute marked “required” must occur exactly once.

## `service`

```hcl
service "NAME" {
  description = "PUBLIC_DESCRIPTION"
  behaviors = ["PATH", ...]

  listen {
    host = "IP_ADDRESS"
    port = PORT
  }

  openapi {
    path = "PATH"
  }
}
```

The block is the composition root for a runnable service.

| Member | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | non-empty string | — | Service name. It must match the name used by `register`, `update`, or `start`. |
| `description` | no | non-empty string | — | Public human-readable service description retained for `simile describe`. |
| `behaviors` | no | array of strings | `[]` | Local behavior-module paths, resolved relative to the configuration file. |
| `listen` | yes | one block | — | Listener configuration. |
| `openapi`, `smithy`, or `typescript` | yes | exactly one of these blocks | — | Public HTTP contract source. |

Exactly one `service` block, one `listen` block, and one contract block are required.

### `listen`

```hcl
listen {
  host = "127.0.0.1"
  port = 4242
}
```

| Attribute | Required | Type | Default | Constraint |
| --- | --- | --- | --- | --- |
| `host` | no | non-empty string | `"127.0.0.1"` | Must parse as an IP address when the service starts. |
| `port` | yes | integer | — | `1` through `65535`. |

The CLI `simile start ... --port PORT` overrides `listen.port` for that invocation without changing
the configuration.

### `openapi`, `smithy`, and `typescript`

Exactly one source attribute is required:

```hcl
openapi { path = "./commerce.yml" }
```

```hcl
openapi { url = "https://example.test/commerce.yml" }
```

```hcl
smithy { path = "./commerce.smithy" }
typescript { path = "./typescript/commerce.d.ts" }
```

| Attribute | Type | Meaning |
| --- | --- | --- |
| `path` | non-empty string | Local contract file, or TypeScript declaration package entrypoint/directory. Relative paths resolve from the configuration directory. |
| `url` | non-empty string | HTTP or HTTPS single-source contract read into memory during compilation. |

`path` and `url` are mutually exclusive. The three contract blocks are mutually exclusive. A
service configuration loaded from a URL has no local base directory, so it must use a contract URL
and inline behavior declarations. Split TypeScript packages require a local path; a TypeScript URL
must identify a self-contained declaration document.

## Behavior declarations

The remaining top-level blocks are legal both inline in a service configuration and in a behavior
module.

### `resource`

```hcl
resource "KIND" "NAME" {
  description = "PUBLIC_DESCRIPTION"
}
```

A resource names an abstract capability used by emulated asynchronous work. `description` is the
only optional attribute.
The supported kinds are:

| Kind | Capability |
| --- | --- |
| `db` | durable record storage |
| `cache` | temporary keyed storage |
| `queue` | queued work delivery |
| `mail` | mail delivery |
| `object_store` | object or blob storage |
| `event_bus` | event publication |
| `scheduler` | delayed or scheduled work |
| `http` | an outbound HTTP dependency |

The canonical resource reference is `KIND.NAME`, for example `queue.orders`. Resource kinds are
provider-neutral; names such as `postgres`, `redis`, or `s3` are rejected.

### `async`

```hcl
async "NAME" {
  description = "PUBLIC_DESCRIPTION"
  uses = ["RESOURCE_REFERENCE", ...]

  action "RESOURCE_REFERENCE" "COMMAND" {
    ARGUMENT = VALUE
  }
}
```

| Attribute | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `description` | no | non-empty string | — | Public explanation of the planned work. |
| `uses` | no | array of strings | `[]` | Resource references required by the planned work. |
| `action` | no | zero or more blocks | — | Typed resource commands dispatched outside the response-critical path. |

Every reference must resolve to a declared resource. Action resources are added to `uses`
automatically. An async declaration is inert until an `effect.async` attribute names it; once
attached, the service supervisor dispatches its actions on a background worker without making
response delivery wait for them.

### `effect`

```hcl
effect "NAME" {
  description = "PUBLIC_DESCRIPTION"
  operations  = ["OPERATION_ID", ...]
  async       = ["ASYNC_NAME", ...]

  action "RESOURCE_REFERENCE" "COMMAND" {
    ARGUMENT = VALUE
  }
}
```

| Member | Required | Type or multiplicity | Meaning |
| --- | --- | --- | --- |
| `description` | no | non-empty string | Public explanation of the consequence. |
| `operations` | yes | non-empty array | Exact contract `operationId` values that commit the effect. |
| `async` | no | array | Named async units dispatched outside the response-critical path. |
| `action` | no | zero or more blocks | Synchronous typed commands committed before delivery. |

An effect must contain at least one synchronous action or one async reference. Operation IDs,
async names, resources, commands, required arguments, and accepted optional arguments are all
validated at compile time.

An action value may be a JSON-compatible literal, `request`, `response`, a dotted projection such
as `request.customer_id` or `response.order.id`, or an array/object recursively containing those
values. Projections are compiled into JSON Pointers; arbitrary functions and private resource reads
are not part of the expression language.

Supported mutation commands are:

| Resource | Commands |
| --- | --- |
| `db` | `put`, `delete` |
| `cache` | `set`, `delete`, `increment`, `compare_and_set` |
| `queue` | `publish` |
| `mail` | `send` |
| `object_store` | `put`, `delete` |
| `event_bus` | `publish` |
| `scheduler` | `schedule`, `cancel` |
| `http` | `register`, `call` |

The command determines required and optional arguments. For example, `db.put` requires
`collection`, `key`, and `value`; `queue.publish` requires `body` and optionally accepts
`attributes` and `delay_ms`. Unsupported reads such as `db.get` are rejected because an effect
describes consequences, not an alternate query API.

### `behavior`

```hcl
behavior "OPERATION_ID" {
  description = "PUBLIC_DESCRIPTION"
  error_sets = ["ERROR_SET", ...]

  response { status = STATUS }
  outcome "NAME" { ... }
}
```

The label is an exact OpenAPI `operationId`. Each operation may have at most one behavior across the
complete service.

| Member | Required | Multiplicity | Meaning |
| --- | --- | --- | --- |
| `description` | no | one string | Public explanation of the operation behavior. |
| `error_sets` | no | one array | Reusable outcome families attached to this operation. |
| `response` | no | zero or one | Default response-status preference. |
| `outcome` | no | zero or more | Provider-authored named result. |

All referenced error sets must exist. All selected statuses must already be declared by the
OpenAPI operation.

### `response`

```hcl
response {
  status = 201
}
```

`status` is required and must be an HTTP status integer from `100` through `599`. The operation's
OpenAPI responses must declare it.

### `outcome`

```hcl
outcome "manual_review" {
  description  = "The public result while the order awaits review."
  status       = 202
  content_type = "application/json"
  body = {
    status = "requires_review"
  }

  when {
    path   = "/delivery/method"
    equals = "manual"
  }
}
```

| Member | Required | Type or multiplicity | Meaning |
| --- | --- | --- | --- |
| label | yes | string | Public outcome name. |
| `description` | no | non-empty string | Public explanation of the result. |
| `status` | yes | integer `100`–`599` | Response status already declared by OpenAPI. |
| `content_type` | no | string | Response media type. |
| `body` | no | JSON-compatible HCL value | Exact provider-authored response body. |
| `when` | no | zero or more blocks | Public request predicates. |

A supplied body must validate against the selected OpenAPI response schema. A provider-authored
outcome may replace an OpenAPI-derived outcome with the same name. Two different provider-authored
outcomes with the same name for one operation are a compilation error.

#### `when`

```hcl
when {
  path   = "/payment_method/type"
  equals = "mobile_money"
}
```

`path` is a JSON Pointer into the validated request body. `equals` is a JSON-compatible value.
Every `when` block on an outcome must match for the outcome to be selected. Predicates operate only
on public request data; they cannot read headers, production state, resources, or private rules.

### `error_set`

```hcl
error_set "payment_errors" {
  description = "Public payment failures shared by payable operations."
  outcome "card_declined" {
    status = 402
    body = {
      error = { code = "card_declined" }
    }
  }
}
```

An error set is a reusable collection of `outcome` blocks. Its optional `description` is a non-empty
public string. A service may declare each error-set name once. An error set is inert until a
`behavior.error_sets` attribute or `apply_error_set` block attaches it to operations.

### `apply_error_set`

```hcl
apply_error_set "payment_errors" {
  tags = ["payments", "invoices"]
}
```

`tags` is a required, non-empty array of OpenAPI tag strings. The named error set is attached to
every operation that has at least one listed tag. Every named tag and the error set must exist.
Each attached outcome is checked against every affected operation's declared responses.

### `protocol_error`

```hcl
protocol_error "invalid_body" {
  description  = "The submitted body does not satisfy the operation contract."
  statuses     = [422, 400]
  content_type = "application/json"
  body = {
    error = {
      code    = "invalid_body"
      message = "The request body is invalid"
    }
  }
}
```

| Attribute | Required | Type | Meaning |
| --- | --- | --- | --- |
| `description` | no | non-empty string | Public explanation of the protocol failure. |
| `statuses` | yes | non-empty array of integers `100`–`599` | Acceptable service statuses, in preference order. |
| `content_type` | no | string | Response media type. |
| `body` | no | JSON-compatible HCL value | Exact error body. |

Simile defines these provider-neutral causes: `malformed_request`, `invalid_body`,
`unsupported_media_type`, `authentication_required`, `permission_denied`,
`idempotency_conflict`, and `rate_limited`. Declaring the same cause replaces Simile's built-in
mapping for the service. At runtime, Simile chooses only a compatible response declared by the
matched operation; otherwise it uses its diagnostic fallback.

### `scenario`

```hcl
scenario "decline_after_delay" {
  description = "Return the public decline outcome after a slow response."
  select "createPayment" {
    outcome = "card_declined"
  }

  inject "createPayment" {
    fault = "http.slow"
  }
}
```

A scenario is an opaque, deterministic test control. Its optional `description` is retained for
`simile describe`. Its label is selected with `--scenario`.

| Nested block | Label | Required attribute | Meaning |
| --- | --- | --- | --- |
| `select` | OpenAPI `operationId` | `outcome` string | Select a named outcome. |
| `inject` | OpenAPI `operationId` | `fault` string | Apply a named environmental fault. |

One scenario may contain selections and injections for several operations. It may select and inject
for the same operation, but it may not contain two selections or two injections for one operation.
All operation IDs, outcomes, and faults must resolve during compilation.

## Environmental faults

Domain failures belong in outcomes. Faults model how a completed result reaches the caller and may
be activated deterministically by a scenario or stochastically by a fault profile.

### `faults` and `use`

```hcl
faults {
  use "http.slow" {}
  use "http.response_lost" {}
}
```

The plural block imports complete definitions from Simile's built-in fault catalog.

```hcl
faults {
  use "*" {}
  use "^http.response_lost" {}
}
```

Selectors have four forms:

| Form | Meaning |
| --- | --- |
| `name` | Import one exact catalog fault. |
| `namespace.*` | Import every fault in a namespace. |
| `*` | Import the complete catalog. |
| `^selector` | Exclude an exact fault or namespace selected anywhere in the service. |

An unmatched selector is an error. Repeated imports of the same unchanged definition are
idempotent. Exclusions are applied across all modules before positive imports.

An exact selector may be aliased or receive a supported override:

```hcl
faults {
  use "http.slow" {
    as      = "checkout_timeout"
    latency = "5s"
  }
}
```

| Attribute | Required | Meaning |
| --- | --- | --- |
| `as` | no | Non-empty local fault name. |
| `latency` | no | Whole-number duration from `1ms` through `24h`; units are `ms`, `s`, `m`, or `h`. |

Aliases and overrides are legal only when the selector resolves to one fault. Exclusions accept no
attributes. An override is legal only when that catalog action supports it. The current catalog is
listed in the [fault glossary](/fault-glossary).

### `fault`

```hcl
fault "slow_response" {
  description = "Delay the completed response by 250 milliseconds."
  phase = "before_response"

  delay {
    milliseconds = 250
  }
}
```

```hcl
fault "response_lost" {
  phase = "after_commit"
  disconnect {}
}
```

A custom fault has one label, an optional non-empty `description`, a required `phase`, and exactly
one action block. Imported catalog faults inherit the catalog's built-in description, including
when aliased.

| Phase | Legal action | Meaning |
| --- | --- | --- |
| `before_response` | `delay` | Wait before sending the completed response. |
| `after_commit` | `disconnect` | Close delivery after the operation has committed. |

`delay.milliseconds` is a required integer from `1` through `86,400,000`. Other phase/action
combinations are rejected.

### `fault_profile`

```hcl
fault_profile "rough_network" {
  description       = "Occasionally exercise slow or lost responses."
  probability       = 0.02
  faults            = ["http.slow", "http.response_lost"]
  operations        = ["createOrder", "createPayment"]
  max_consecutive   = 1
  cooldown_requests = 20
  seed              = 42
}
```

| Attribute | Required | Type | Default | Constraint |
| --- | --- | --- | --- | --- |
| `description` | no | non-empty string | — | Public explanation of the stochastic test policy. |
| `probability` | yes | number | — | `0.000001` through `1`; stored as parts per million. |
| `faults` | yes | non-empty array of strings | — | Each fault must exist and occur once. |
| `operations` | no | array of strings | `[]` | OpenAPI `operationId` allowlist; empty means every operation. |
| `max_consecutive` | no | unsigned integer | `1` | Must be positive. |
| `cooldown_requests` | no | unsigned integer | `20` | Requests suppressed after activation bounds are reached. |
| `seed` | no | unsigned integer | random | Reproducible stochastic sequence. |

Profiles may contain environmental faults only. Domain outcomes are deterministic and cannot be
randomly selected by a fault profile.

## Name resolution and compilation

Parsing proves that each file uses legal HCL structure and value types. Compilation then combines
all modules with OpenAPI and resolves the service as a whole.

Resolution is independent of declaration order. A reference may target a declaration in a later
file. Compilation rejects:

- duplicate resources, behaviors, error sets, async operations, scenarios, custom faults, or fault
  profiles;
- unknown OpenAPI operation IDs or tags;
- response statuses absent from the targeted OpenAPI operation;
- provider-authored response bodies that fail the selected response schema;
- unknown resources, error sets, outcomes, or faults;
- duplicate provider-authored outcome names for one operation; and
- incompatible fault phases, actions, selectors, or stochastic-profile members.

Successful compilation produces a deterministic, self-contained `.similec` artifact. Imported
catalog faults are expanded into the artifact; the runtime does not need the original HCL or fault
catalog to execute it.

## Privacy and publication boundary

Simile HCL is distributable and inspectable. It must describe public observable behavior, not
private implementation decisions. Use public request predicates when an input deliberately triggers
an outcome. Use an opaque scenario when the underlying production decision must remain private.
Resource declarations name capabilities, not vendors, credentials, private tables, thresholds, or
topology.

## Language evolution

The Simile HCL language currently versions with Simile rather than through a separate language
version attribute. A released Simile binary is the authority for the grammar it accepts. New
optional blocks or attributes may be added compatibly; incompatible syntax or semantic changes
require an appropriate Simile release and migration notes.

For terminology, read the [HCL glossary](/hcl-glossary). For source ownership and design rationale,
read [OpenAPI and HCL](/openapi-and-hcl).
