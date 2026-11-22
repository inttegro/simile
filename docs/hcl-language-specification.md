---
title: Language specification
sidebarTitle: Spec
description: The complete syntax and semantics of the Simile HCL language.
---

This document defines the Simile HCL language implemented by Simile v0.3.0. It is the canonical
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
in the same file. `Simfile` is the conventional filename for a local service configuration:

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
same compiler and namespace. `simile register`, `simile describe`, and `simile start` load
`./Simfile` when invoked without a service name or explicit configuration.

### Behavior module

A behavior module contains only behavior declarations. It has no `service` block:

```hcl
backend "queue" "orders" {}

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
| [`principal_setup`](#principal_setup) | once per principal | not allowed | Define how Simile provisions a principal profile. |
| [`backend`](#backend) | optional | optional | Name an emulated backing system. |
| [`principal`](#principal) | optional | optional | Declare a kind of contract-authenticated caller. |
| [`profile`](#profile) | optional | optional | Declare a concrete fixture identity for one principal. |
| [`async`](#async) | optional | optional | Declare planned follow-up work. |
| [`behavior`](#behavior) | optional | optional | Refine one contract operation. |
| [`error_set`](#error_set) | optional | optional | Define reusable named outcomes. |
| [`apply_error_set`](#apply_error_set) | optional | optional | Attach an error set through contract tags. |
| [`protocol_error`](#protocol_error) | optional | optional | Map a runtime failure cause into service responses. |
| [`scenario`](#scenario) | optional | optional | Define deterministic outcome and fault selections. |
| [`faults`](#faults-and-use) | optional | optional | Import built-in environmental faults. |
| [`fault`](#fault) | optional | optional | Define a custom environmental fault. |
| [`fault_profile`](#fault_profile) | optional | optional | Define bounded stochastic fault activation. |
| [`webhook`](#webhook) | optional | optional | Declare a provider-authored outbound webhook missing from the contract. |
| [`webhooks`](#webhooks) | at most one across all sources | at most one across all sources | Configure ordinary webhook delivery ordering and concurrency. |

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

Named semantic blocks accept an optional `description` attribute: `service`, `principal_setup`,
principal-setup `collect` and `issue`, `backend`, `principal`, `profile`, `async`,
`effect`, `behavior`, `outcome`, `error_set`, `protocol_error`, `scenario`, custom `fault`,
`fault_profile`, and provider-authored `webhook`. Its value must be a non-empty string. Descriptions are presentation metadata
retained in the compiled artifact and displayed by `simile describe service`; they never influence
matching, selection, synthesis, or execution.

Structural blocks do not accept `description`: `listen`, `openapi`, `response`, `when`,
`apply_error_set`, `faults`, `webhooks`, `use`, `select`, `inject`, `delay`, and `disconnect`. Their meaning
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

  reporting {
    endpoint = "HTTPS_URL"
  }

  openapi {
    path = "PATH"
  }
}
```

The block is the composition root for a runnable service.

| Member | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | non-empty string | — | Service name. Local `Simfile` commands derive the name from this label; an explicitly supplied name must match it. |
| `description` | no | non-empty string | — | Public human-readable service description retained for `simile describe`. |
| `behaviors` | no | array of strings | `[]` | Local behavior-module paths, resolved relative to the configuration file. |
| `listen` | yes | one block | — | Listener configuration. |
| `reporting` | no | one block | — | Provider report-receiver destination. User consent is configured separately per profile. |
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

### `reporting`

```hcl
reporting {
  endpoint = "https://reports.example.com/v1/reports"
}
```

| Attribute | Required | Type | Constraint |
| --- | --- | --- | --- |
| `endpoint` | yes | non-empty string | Absolute HTTPS URL with a non-root path and no query or trailing slash. Loopback HTTP is accepted for local receiver development. |

The endpoint declares provider capability. It grants no permission to submit. `simile setup` and
`simile reporting` store user-owned automatic-reporting consent and contact information outside the
service artifact. One-off reports require an explicit send action and contact address.

Simile appends `/preflight` to this URL for version and service-definition checks. It sends the
technical report, reproduction attachments, and reporter contact as separate multipart fields to
the declared endpoint. See [Report an issue](/issue-reporting).

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

## `principal_setup`

```hcl
principal_setup "application" {
  description = "Create one local application identity."
  operation   = "createApplication"

  collect "name" {
    description = "A name that identifies the integration."
    required    = true
    type        = "string"
    to          = request.name
  }

  collect "webhook_url" {
    type = "url"
    to   = request.webhook_url
    bind = webhook.target
  }

  issue "app_id" {
    from = response.app.id
  }

  issue "access_token" {
    from       = response.access_token
    credential = "merchantOAuth"
    sensitive  = true
  }

  issue "webhook_secret" {
    generate  = "token"
    prefix    = "whsec_simile_"
    bind      = webhook.signing_secret
    sensitive = true
  }
}
```

The block may repeat in a service configuration and is rejected in behavior modules. Its label
names the declared `principal` it provisions. Each principal may have at most one
`principal_setup` block. At
least one `issue` block is required.

| Member | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | declared principal name | — | Principal provisioned by this workflow. |
| `description` | no | non-empty string | — | Public explanation shown by `simile describe service`. |
| `operation` | yes | non-empty string | — | Exact operation ID executed when creating a profile. |
| `collect` | no | repeated block | — | Caller-supplied setup input. Names must be unique. |
| `issue` | yes | repeated block | — | Value retained and made available to the caller. Names must be unique. |

The referenced operation must exist in the selected contract. Setup synthesizes its smallest valid
request, applies collected values, and selects a declared successful response when one is
available. The operation's synchronous effects and profile creation commit in one service-state
checkpoint.

### `collect`

| Member | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | non-empty string | — | Input name used by prompts and `--set NAME=VALUE`. |
| `description` | no | non-empty string | — | Public help text shown during interactive setup. |
| `required` | no | boolean | `false` | Whether setup may proceed without the value. |
| `type` | no | string enum | `"string"` | `string`, `url`, `boolean`, `integer`, or `json`. |
| `to` | no | `request.FIELD...` traversal | `request.<label>` | Destination in the operation request body. |
| `bind` | no | supported binding | — | Retain the value as runtime profile configuration. |

Supported collect bindings are `webhook.target` and `webhook.signing_secret`. A URL value must
begin with `http://` or `https://`. Traversal segments are compiled to JSON Pointer; quoted index
syntax may address property names that are not HCL identifiers.

### `issue`

| Member | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | non-empty string | — | Issued-value name and environment-variable source. |
| `description` | no | non-empty string | — | Public explanation shown by service description. |
| `from` | exactly one source | `response.FIELD...` traversal | — | Extract the value from the setup-operation response. |
| `generate` | exactly one source | `"token"` | — | Generate a high-entropy local value. |
| `prefix` | no | string | `""` | Prefix for a generated token; invalid with `from`. |
| `credential` | no | principal credential name | — | Fill this credential slot on the resulting profile. |
| `sensitive` | no | boolean | `false` | Mark the value as secret presentation data. |
| `bind` | no | supported binding | — | Retain the value as runtime profile configuration. |

`from` and `generate` are mutually exclusive. A `credential` must name a slot declared by the
referenced principal, whose label in turn names a supported contract security scheme. API key,
HTTP bearer, OAuth 2.0, and OpenID Connect credentials use one issued string;
HTTP Basic credentials require a username/password pair and therefore cannot currently be issued
by setup. Credential values must be strings.

Supported issue bindings are `webhook.target` and `webhook.signing_secret`. Sensitive values are
stored for runtime use and written to the profile's private environment file; describe, profile,
and setup reports omit their contents.

## `webhook`

OpenAPI callbacks and OpenAPI 3.1/3.2 top-level webhooks are imported as webhook contracts. Use a
singular `webhook` block only when that public contract is absent from the selected API source:

```hcl
webhook "payment.succeeded" {
  description  = "Sent after a payment has committed successfully."
  method       = "POST"
  content_type = "application/json"
}
```

| Member | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | non-empty string | — | Stable name referenced by `effect.emit`. |
| `description` | no | non-empty string | — | Public explanation retained for `simile describe`. |
| `method` | no | HTTP method token | `"POST"` | Method used by the outbound message. |
| `content_type` | no | non-empty string | `"application/json"` | Media type assigned to the provider-authored payload. |

The block defines a wire-message identity, not a trigger. An [effect](#effect) decides which
committed operation emits it and supplies the payload. A provider-authored webhook cannot redefine
a webhook imported from the API contract. Its payload is not schema-constrained; publish the
webhook in OpenAPI when consumers need Simile to validate its body against a reusable schema.

## `webhooks`

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
```

The block configures the service's webhook delivery infrastructure. It does not declare a webhook
payload or decide which operation emits one. At most one `webhooks` block may appear across the
inline configuration and every imported behavior module.

| Attribute | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `ordering` | no | `"best_effort"` or `"strict"` | `"best_effort"` | Whether independent deliveries may overtake one another. |
| `out_of_order_probability` | no | number from `0` through `1` | `0.10` for best effort; `0` for strict | Probability that one eligible later delivery overtakes an earlier delivery. |
| `reorder_window` | no | duration from `1ms` through `24h` | `"5s"` for best effort; disabled for strict | Maximum scheduled-time separation within which reordering may occur. |
| `concurrency` | no | positive integer through `65535` | `8` | Maximum concurrent delivery workers represented by the policy. |

### `signing`

```hcl
signing {
  secret           = "whsec_simile_local"
  header           = "X-Simile-Signature"
  timestamp_header = "X-Simile-Timestamp"
}
```

| Attribute | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `secret` | yes | non-empty string | — | Local test key used to sign the timestamp and exact request body with HMAC-SHA256. |
| `header` | no | non-empty string | `"X-Simile-Signature"` | Header carrying `t=TIMESTAMP,v1=HEX_DIGEST`. |
| `timestamp_header` | no | non-empty string | `"X-Simile-Timestamp"` | Header carrying the Unix timestamp included in the signature. |

The signing key becomes part of the local compiled artifact so that restarts reproduce the same
delivery behavior. Use a dedicated test value. Production credentials do not belong in a Simile
configuration.

`best_effort` is deliberately the default. Concurrent webhook infrastructure rarely guarantees
global ordering, and retries can naturally arrive after newer occurrences. Simile samples only
independent deliveries inside the configured window. The decision is derived from the compiled
service fingerprint, correlation identifier, and delivery identifiers, so restarting the same
service reproduces the same decision.

An explicit causal dependency is stronger than this policy. When one occurrence declares that it
must follow another, the runtime never reorders that pair. A strict service must omit
`reorder_window` and set `out_of_order_probability` to zero or omit it. A non-zero probability also
requires concurrency of at least two.

See [Webhook delivery](/webhook-delivery) for the policy layers and rationale.

## Behavior declarations

The remaining top-level blocks are legal both inline in a service configuration and in a behavior
module.

### `backend`

```hcl
backend "KIND" "NAME" {
  description = "PUBLIC_DESCRIPTION"
}
```

A backend names an emulated backing system used by synchronous effects or asynchronous work.
`description` is the only optional attribute.
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

The canonical backend reference is `KIND.NAME`, for example `queue.orders`. Backend kinds are
provider-neutral. Names such as `postgres`, `redis`, or `s3` are rejected.

### `principal`

```hcl
principal "application" {
  description = "An application that can create orders."

  credential "merchant_key" {
    grants = ["orders:write"]
  }

  credential "operator_login" {}
}
```

A principal describes one kind of service caller. Each credential label names a security scheme
from the selected contract. `grants` belongs to the credential slot because an OpenAPI operation
requests scopes through a particular scheme.

| Member | Required | Type or multiplicity | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | non-empty string | — | Stable caller-kind name shown by `simile describe service`. |
| `description` | no | non-empty string | — | Public purpose of this caller kind. |
| `credential` | no | repeated block | — | Credential slot named after a contract security scheme. |

Each principal `credential` accepts an optional `grants` array. Credential values live on profiles.
The compiler verifies that each credential label resolves to a contract security scheme. During
authentication, Simile compares an operation's required scopes with the grants on that slot.

### `profile`

```hcl
profile "checkout_test" {
  description = "A checkout fixture with active credentials."
  principal   = principal.application

  credential "merchant_key" {
    value = "sk_simile_checkout"
  }

  credential "operator_login" {
    username = "checkout"
    password = "simile"
  }
}
```

A profile is one concrete instance of a principal. Profiles declared in HCL are fixture identities
compiled into the artifact. `simile setup` creates durable profiles through a `principal_setup`
workflow.

| Member | Required | Type or multiplicity | Default | Meaning |
| --- | --- | --- | --- | --- |
| label | yes | non-empty string | — | Profile name shown by description and audit tools. |
| `description` | no | non-empty string | — | Public purpose of the fixture. |
| `principal` | yes | `principal.NAME` reference | — | Principal contract instantiated by the profile. |
| `status` | no | `"active"`, `"revoked"`, or `"expired"` | `"active"` | Authentication state for this profile. |
| `credential` | no | repeated block | — | Concrete value for a slot declared by the principal. |

A profile `credential` accepts either `value`, or the pair `username` and `password`. API-key,
bearer, OAuth2, and OpenID schemes use `value`; HTTP Basic uses the username/password pair. The
compiler checks the credential form against the referenced security scheme. Use synthetic values
created solely for emulation.

OpenAPI security requirements preserve their native composition. Schemes inside one requirement
are all required; separate requirements are alternatives. An operation with an empty security
requirement remains public. Simile authenticates an active profile, then evaluates grants through
its principal contract. Missing, invalid, revoked, expired, or insufficient credentials select the
operation's declared authentication or permission response.

### `async`

```hcl
async "NAME" {
  description = "PUBLIC_DESCRIPTION"
  uses = ["BACKEND_REFERENCE", ...]

  action "BACKEND_REFERENCE" "COMMAND" {
    ARGUMENT = VALUE
  }
}
```

| Attribute | Required | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `description` | no | non-empty string | — | Public explanation of the planned work. |
| `uses` | no | array of strings | `[]` | Backend references required by the planned work. |
| `action` | no | zero or more blocks | — | Typed backend commands dispatched outside the response-critical path. |

Every reference must resolve to a declared backend. Action backends are added to `uses`
automatically. An async declaration is inert until an `effect.async` attribute names it; once
attached, the service writes the planned work into its durable outbox in the same commit as the
operation. A background worker leases and dispatches the actions after the response-critical work
has committed. Restarting a persistent service preserves pending work, lease expiry makes an
interrupted item eligible again, and exhausted attempts move the item to the dead-letter queue.

### `effect`

```hcl
effect "NAME" {
  description = "PUBLIC_DESCRIPTION"
  operations  = ["OPERATION_ID", ...]
  async       = ["ASYNC_NAME", ...]

  action "BACKEND_REFERENCE" "COMMAND" {
    ARGUMENT = VALUE
  }

  emit = {
    "payment.succeeded" = response.payment_event
    "order.paid"        = response.order_event
    "receipt.available" = response.receipt_event
  }

  after = {
    "order.paid" = ["payment.succeeded"]
  }

  delay = {
    "receipt.available" = "2s"
  }

  to = {
    "payment.succeeded" = request.callback_url
  }
}
```

| Member | Required | Type or multiplicity | Meaning |
| --- | --- | --- | --- |
| `description` | no | non-empty string | Public explanation of the consequence. |
| `operations` | yes | non-empty array | Exact contract `operationId` values that commit the effect. |
| `async` | no | array | Named async units committed to the durable outbox and dispatched in the background. |
| `action` | no | zero or more blocks | Synchronous typed commands committed before delivery. |
| `emit` | no | object | Webhook name to request/response-derived payload. |
| `after` | no | object | Emitted webhook name to hard prerequisites from the same effect. |
| `delay` | no | object | Emitted webhook name to non-negative delivery delay. |
| `to` | no | object | Emitted webhook name to an explicit destination projection or string. |

An effect must contain at least one synchronous action, one async reference, or one webhook
emission. Operation IDs, async names, webhook names, backends, commands, required arguments, and
accepted optional arguments are all validated at compile time.

An action value may be a JSON-compatible literal, `request`, `response`, a dotted projection such
as `request.customer_id` or `response.order.id`, or an array/object recursively containing those
values. Projections are compiled into JSON Pointers; arbitrary functions and private backend reads
are not part of the expression language.

`emit` is deliberately a map rather than a repeated block: one effect can declare several related
occurrences compactly. Every key must name a webhook imported from OpenAPI or declared with
`webhook`. An OpenAPI webhook body is checked against its request schema after the payload
projection is rendered. `after`, `delay`, and `to` are optional maps whose keys must also occur in
`emit`; they refine only the named occurrence.

`after` forms a hard causal graph. Dependencies must name distinct webhook occurrences emitted by
the same effect, and cycles are rejected. The service-wide best-effort delivery policy may reorder
independent occurrences but never crosses one of these edges. `delay` accepts the same duration
syntax as other HCL timing controls. `to` accepts a string or request/response projection and
overrides an OpenAPI callback runtime expression for that occurrence.

An OpenAPI callback target is otherwise resolved from its runtime expression. The current runtime
supports JSON-body pointers such as `{$request.body#/callback_url}` and
`{$response.body#/links/webhook}`. Header, query, path, and expression-composition targets are
retained in the contract but require an explicit `to` override until those parameter contexts are
available to effect planning.

Supported mutation commands are:

| Backend | Commands |
| --- | --- |
| `db` | `put`, `merge`, `delete` |
| `cache` | `set`, `delete`, `increment`, `compare_and_set` |
| `queue` | `publish` |
| `mail` | `send` |
| `object_store` | `put`, `delete` |
| `event_bus` | `publish` |
| `scheduler` | `schedule`, `cancel` |
| `http` | `register`, `call` |

The command determines required and optional arguments. For example, `db.put` and `db.merge`
require `collection`, `key`, and `value`; `queue.publish` requires `body` and optionally accepts
`attributes` and `delay_ms`. `db.put` replaces the stored document. `db.merge` recursively merges
object fields, replaces supplied scalars and arrays, and preserves omitted fields. Unsupported reads
such as `db.get` are rejected because an effect describes consequences, not an alternate query API.

### `behavior`

```hcl
behavior "OPERATION_ID" {
  description = "PUBLIC_DESCRIPTION"
  error_sets = ["ERROR_SET", ...]

  response { status = STATUS }
  resolve "db.BACKEND" { ... }
  outcome "NAME" { ... }
}
```

The label is an exact contract operation ID. Each operation may have at most one behavior across the
complete service.

| Member | Required | Multiplicity | Meaning |
| --- | --- | --- | --- |
| `description` | no | one string | Public explanation of the operation behavior. |
| `error_sets` | no | one array | Reusable outcome families attached to this operation. |
| `response` | no | zero or one | Default response-status preference. |
| `resolve` | no | zero or more | Resolve a stored public reference for response synthesis. |
| `outcome` | no | zero or more | Provider-authored named result. |

All referenced error sets must exist. All selected statuses must already be declared by the
contract operation.

### `resolve`

```hcl
behavior "createOrder" {
  resolve "db.resources" {
    collection = "prices"
    key        = "/line_items/*/product/price_id"
    select     = "/fixed_amount"
    into       = "/line_items/*/product/price"
    missing_outcome = "price_not_found"

    where {
      stored  = "/product_id"
      request = "/line_items/*/product/product_id"
      otherwise = "invalid_line_items"
    }
    where {
      stored = "/active"
      equals = true
      otherwise = "price_inactive"
    }
    where {
      stored = "/archived_at"
      state  = "null-or-absent"
    }
  }
}
```

`resolve` reads one document for each scalar selected by `key`, takes the stored value at `select`,
and adds it at `into` in the request view used by response relations. It does not modify the
validated request retained by the operation, outcome predicates, replay, or effects. This makes it
safe to enrich a reference branch with a field that is mutually exclusive on the public wire.

| Member | Required | Meaning |
| --- | --- | --- |
| label | yes | Declared `db.NAME` backend used for the read. |
| `collection` | yes | Public resource collection written by an effect. |
| `key` | yes | Request JSON Pointer selecting stored keys; complete `*` segments iterate arrays. |
| `select` | yes | JSON Pointer into each stored public document. Wildcards are not allowed. |
| `into` | yes | Request-view JSON Pointer receiving the selected value. It must have the same wildcard count as `key`. |
| `missing_outcome` | no | Provider-authored outcome selected when the stored public document does not exist. |
| `where` | no | Public predicates a stored document must satisfy before projection. |

Every `where` block requires `stored` and exactly one predicate:

- `equals = VALUE` compares a stored field with a JSON-compatible literal;
- `request = "/path"` compares it with the request value at the corresponding wildcard position;
  or
- `state = "present|absent|null|non-null|null-or-absent"` checks presence and nullability.

`otherwise` may name a provider-authored outcome on any `where` block. The named outcome must
belong to the same behavior or one of its applied error sets. Predicates are evaluated in source
order, so authors can preserve a documented precedence between overlapping public lifecycle states.

State projections must use public resource names and public fields. They are not a way to publish
private tables, thresholds, routing rules, or production decision trees. A missing document,
missing selected value, or failed predicate cannot be used to construct a successful response.
Without an explicit failure outcome, those conditions remain provider-configuration errors.

### `response`

```hcl
response {
  status = 201
}
```

`status` is required and must be an HTTP status integer from `100` through `599`. The operation's
contract responses must declare it.

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
| `status` | yes | integer `100`–`599` | Response status already declared by the contract. |
| `content_type` | no | string | Response media type. |
| `body` | no | JSON-compatible HCL value | Exact provider-authored response body. |
| `when` | no | zero or more blocks | Public request predicates. |

A supplied body must validate against the selected contract response schema. A provider-authored
outcome may replace a contract-derived outcome with the same name. Two different provider-authored
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
on public request data; they cannot read headers, production state, backends, or private rules.

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

`tags` is a required, non-empty array of contract tag strings. The named error set is attached to
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
| `select` | Contract operation ID | `outcome` string | Select a named outcome. |
| `inject` | Contract operation ID | `fault` string | Apply a named environmental fault. |

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
| `operations` | no | array of strings | `[]` | Contract operation-ID allowlist; empty means every operation. |
| `max_consecutive` | no | unsigned integer | `1` | Must be positive. |
| `cooldown_requests` | no | unsigned integer | `20` | Requests suppressed after activation bounds are reached. |
| `seed` | no | unsigned integer | random | Reproducible stochastic sequence. |

Profiles may contain environmental faults only. Domain outcomes are deterministic and cannot be
randomly selected by a fault profile.

## Name resolution and compilation

Parsing proves that each file uses legal HCL structure and value types. Compilation then combines
all modules with the selected contract and resolves the service as a whole.

Resolution is independent of declaration order. A reference may target a declaration in a later
file. Compilation rejects:

- duplicate backends, behaviors, error sets, async operations, scenarios, custom faults, or fault
  profiles;
- unknown contract operation IDs or tags;
- response statuses absent from the targeted contract operation;
- provider-authored response bodies that fail the selected response schema;
- unknown backends, error sets, outcomes, or faults;
- duplicate provider-authored outcome names for one operation; and
- incompatible fault phases, actions, selectors, or stochastic-profile members.

Successful compilation produces a deterministic, self-contained `.similec` artifact. Imported
catalog faults are expanded into the artifact; the runtime does not need the original HCL or fault
catalog to execute it.

## Privacy and publication boundary

Simile HCL is distributable and inspectable. It must describe public observable behavior, not
private implementation decisions. Use public request predicates when an input deliberately triggers
an outcome. Use an opaque scenario when the underlying production decision must remain private.
Backend declarations expose provider-neutral capabilities. Keep vendor names, credentials, private
tables, thresholds, and topology out of public HCL.

## Language evolution

The Simile HCL language currently versions with Simile rather than through a separate language
version attribute. A released Simile binary is the authority for the grammar it accepts. New
optional blocks or attributes may be added compatibly; incompatible syntax or semantic changes
require an appropriate Simile release and migration notes.

For terminology, read the [HCL glossary](/hcl-glossary). For source ownership and design rationale,
read [Contract sources and HCL](/contract-sources-and-hcl).
