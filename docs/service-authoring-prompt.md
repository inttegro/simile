---
title: Authoring prompt
description: Give a coding agent the evidence, constraints, and validation standard needed to produce a faithful Simile service contract.
---

Use this prompt when asking a coding agent to create or improve a service's OpenAPI contract and
Simile configuration. Replace every bracketed value before sending it. Give the agent access to the
service's public documentation, implementation, tests, and existing contract where possible.

The prompt deliberately separates the public HTTP contract, observable behavior, and private
implementation logic. Its required audit and coverage report make uncertainty visible instead of
letting the agent silently invent contract details.

````text
You are designing a production-quality, Simile-compatible public contract for [SERVICE_NAME].

Inputs

- Service source/docs: [PATHS_OR_URLS]
- Existing OpenAPI document, if any: [OPENAPI_PATH_OR_URL]
- Desired service config path: [CONFIG_PATH]
- Desired listener: [HOST]:[PORT]
- Intended users and primary workflows: [WORKFLOWS]

Your goal is to produce the most accurate public OpenAPI specification and the smallest useful Simile service configuration. The result must let integrators validate requests, generate sound requests and responses, exercise meaningful outcomes, and run the service locally without exposing private implementation logic.

Before editing

1. Study the public API documentation, implementation, tests, fixtures, existing schemas, and error handling.
2. Build an operation inventory containing:
   - method and path;
   - stable `operationId`;
   - request parameters and body;
   - success and error responses;
   - content types;
   - public side effects;
   - asynchronous work observable by callers;
   - cross-field relationships;
   - uncertainties or contradictions.
3. Treat implementation behavior as evidence, not permission to publish private details.
4. Do not invent endpoints, statuses, fields, constraints, error codes, or guarantees. Flag unresolved facts explicitly.

OpenAPI requirements

OpenAPI is the complete authority for the HTTP contract. Preserve the existing OpenAPI version when updating a specification. For a new specification, prefer OpenAPI 3.1.1 unless a required feature justifies another Simile-supported 3.x version.

For every operation:

- Provide a stable, unique `operationId`.
- Accurately define path, query, header, and cookie parameters.
- Define request-body presence, requiredness, content types, and schemas.
- Define every documented success and error status.
- Define response headers, content types, and schemas.
- Add realistic named examples for important successes and errors.
- Use reusable `components` for shared objects, errors, parameters, and responses.
- Use tags consistently so reusable Simile error sets can later target product areas.
- Distinguish request and response fields with `readOnly` and `writeOnly`.
- Use accurate primitive types and formats such as `integer`, `int64`, `date-time`, `uuid`, `uri`, and `email`.
- Use `oneOf`, `anyOf`, discriminators, enums, constants, required fields, and nullability to express genuine alternatives precisely.
- Ensure mutually exclusive success and error shapes cannot appear together.
- Use `additionalProperties: false` only when the real service rejects unknown fields.
- Put machine-readable semantics in schemas, formats, examples, and extensions—not only in descriptions.
- Define reusable public error envelopes with real error codes and helpful examples.

Prefer named OpenAPI examples for exact outcomes. They require less Simile-specific configuration and provide stronger fidelity than schema-only synthesis.

Soundness extensions

Use `x-simile-constraints` only for public cross-field relationships ordinary OpenAPI cannot express.

Attach the extension to the narrowest object schema that owns every participating field. Every path is an RFC 6901 JSON Pointer relative to that object.

Supported clauses include:

- `ordered` for chronological or numeric ordering;
- `bounded` for values constrained by other fields;
- `implies` for conditional presence, nullability, or values;
- `equal` for scalar values that must agree;
- `distinct` for unique values selected through arrays;
- `set` for subset, superset, or disjoint scalar arrays;
- `equation` for arithmetic relationships;
- `aggregate` for sums, counts, minima, or maxima over arrays;
- `lifecycle` for finite states and transition timestamps;
- `template` for strings rendered from required scalar fields.

Examples:

```yaml
x-simile-constraints:
  - ordered:
      paths: [/created_at, /published_at, /updated_at]

  - implies:
      when: { path: /active, equals: false }
      then: { all: [{ path: /published_at, state: null-or-absent }, { path: /checkout_enabled, equals: false }] }

  - equation:
      target: /amount_remaining
      value: { subtract: [{ path: /amount_due }, { path: /amount_paid }] }

  - aggregate:
      target: /subtotal
      operation: sum
      source: /lines/data/*/amount
      when: { path: /lines/has_more, equals: false }
```

Constraint rules:

- Use ordinary OpenAPI for field-local validation.
- Ensure every path resolves and every participating type is compatible.
- Ensure required construction sources are available and non-null where necessary.
- Do not encode database structure, fraud thresholds, vendor rules, private decision trees, or undisclosed production invariants.
- If a relationship is not part of the public contract, do not publish it.
- A constraint must support both proof and construction. Do not add inert annotations.

Simile HCL requirements

HCL is optional and additive. Do not duplicate routes, schemas, or response definitions already owned by OpenAPI.

Use HCL only for public observable behavior that OpenAPI cannot express adequately, including:

- selecting a response status already declared by an operation;
- exact named outcomes whose bodies validate against OpenAPI;
- reusable public error families;
- public request predicates;
- opaque test scenarios;
- protocol-error mappings;
- declared asynchronous work and abstract dependencies; and
- environmental delivery faults and bounded stochastic fault profiles.

Prefer OpenAPI named examples before introducing HCL outcomes.

Never reproduce private production decision logic. If a private decision can lead to a publicly observable result, expose an opaque scenario that selects the result:

```hcl
scenario "payment_declined" {
  select "createPayment" {
    outcome = "card_declined"
  }
}
```

Only use public request values in predicates. Represent dependencies with portable capability names such as:

- `db`
- `cache`
- `queue`
- `mail`
- `object_store`
- `event_bus`
- `scheduler`
- `http`

Do not use implementation-specific names such as PostgreSQL, Redis, SQS, or SendGrid.

Remember that the current runtime plans declared async work but does not yet execute those resources. Document this honestly.

Model domain failures and transport faults separately. Invalid parameters, declined payment methods, and forbidden states are deterministic named outcomes selected by public predicates or opaque scenarios. They must never be members of a stochastic profile.

Use faults only for environmental delivery behavior. Prefer Simile's built-in HTTP catalog so a
service does not repeat standard phase and action mechanics:

```hcl
faults {
  use "http.slow" {}
  use "http.response_lost" {}
}
```

Use `"*"` to import the complete catalog, `"namespace.*"` to import a namespace, and a leading
`^` to exclude an exact fault or namespace from the selected set. Prefer the narrowest selection
that expresses the service's testing contract.

Override a catalog default only when the service contract needs it:

```hcl
faults {
  use "http.slow" {
    latency = "2s"
  }
}
```

Define a custom `fault` only when no catalog entry represents the required environmental
behavior. Never use catalog or custom faults to model domain rejection.

Use scenario `inject` blocks for deterministic tests. Use a `fault_profile` only for bounded stochastic resilience testing, with a conservative probability, operation scope, `max_consecutive`, `cooldown_requests`, and an optional reproducible seed. Do not claim that profile probabilities reproduce production measurements unless the provider supplies that evidence.

Service configuration

Produce a minimal composition-root configuration:

```hcl
service "[SERVICE_NAME]" {
  listen {
    host = "[HOST]"
    port = [PORT]
  }

  openapi {
    path = "./openapi.yaml"
  }

  behaviors = [
    "./behaviors.hcl",
  ]
}
```

Rules:

- The file must contain exactly one `service` block.
- The service label must match the CLI service name.
- The `openapi` block must use exactly one of `path` or `url`.
- Omit `behaviors` when no behavior module is needed.
- Relative paths resolve from the local config directory.
- A remotely loaded config must use an OpenAPI URL and inline behavior declarations; it cannot rely on relative files.
- Default to `127.0.0.1` unless public binding is explicitly required.
- Keep the configuration concerned with composition, not endpoint definitions.

Required deliverables

1. A short contract audit:
   - evidence inspected;
   - contradictions found;
   - unresolved questions;
   - assumptions avoided.
2. The completed OpenAPI document.
3. The minimal Simile service configuration.
4. Optional behavior HCL only where justified.
5. An operation coverage table showing:
   - operation;
   - request example;
   - success outcomes;
   - error outcomes;
   - soundness constraints;
   - HCL behavior, if any;
   - remaining schema-synthesized outcomes.
6. A validation report.

Validation

- Parse and lint the OpenAPI document.
- Compile/register the service with Simile.
- Treat every Simile compilation error as a contract defect; do not remove a rule merely to make compilation pass.
- Run representative operations with synthesized requests.
- Run representative operations with supplied valid requests.
- Test malformed and schema-invalid requests.
- Verify that generated responses match exactly one declared response alternative.
- Verify every generated payload against its compiled schema and constraints.
- Verify important errors use provider-authored examples or outcomes rather than random strings.
- Confirm no private implementation details are present in OpenAPI, HCL, examples, metadata, or scenario names.
- Report the exact commands and results.

Quality standard

The finished contract should be useful without HCL, faithful without exposing internals, economical for the provider to maintain, and strict enough that a successful Simile test gives an integrator meaningful confidence—not merely schema-shaped random data.
````

Read [OpenAPI and HCL](/openapi-and-hcl) for the source-ownership model,
[OpenAPI extensions](/openapi-extensions) for the supported constraint vocabulary, and
[Service configuration](/service-configuration) for the configuration grammar.
