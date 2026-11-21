---
title: Authoring prompt
description: Give a coding agent the evidence, source-language rules, and validation standard needed to produce a faithful Simile service contract.
---

Use this prompt when asking a coding agent to create or improve an OpenAPI, Smithy, or semantic
TypeScript contract and its Simile configuration. Replace every bracketed value before sending it.
Give the agent access to public documentation, implementation, tests, fixtures, existing contracts,
and real error behavior where possible.

The prompt treats all three contract sources as first-class standards. It requires the selected
source to be independently complete and keeps public HTTP behavior separate from private
implementation logic.

````text
You are designing a production-quality, Simile-compatible public contract for [SERVICE_NAME].

Inputs

- Contract format: [openapi | smithy | typescript]
- Service source and public documentation: [PATHS_OR_URLS]
- Existing contract, if any: [CONTRACT_PATH_OR_URL]
- Desired contract output: [CONTRACT_OUTPUT]
- Desired service config path: [CONFIG_PATH, normally ./Simfile]
- Desired listener: [HOST]:[PORT]
- Intended users and primary workflows: [WORKFLOWS]

Goal

Produce the most accurate public contract in the selected format and the smallest useful Simile
service configuration. The contract must independently define the complete HTTP surface. It must
let integrators validate requests, synthesize sound requests and responses, exercise meaningful
public outcomes, and run the service locally without exposing private production logic.

Use `Simfile` as the conventional configuration filename unless the repository already establishes
another explicit path.

Do not translate another contract format mechanically. Model the service's public meaning using
the selected language's native types and Simile's public semantic constructs.

Before editing

1. Study public documentation, handlers, domain types, validation, tests, fixtures, existing
   schemas, examples, and error handling.
2. Build an operation inventory containing:
   - method and path;
   - stable operation ID;
   - path, query, header, and cookie parameters;
   - request-body presence, requiredness, content types, and shapes;
   - success and error statuses, headers, content types, and shapes;
   - public side effects and caller-observable asynchronous work;
   - provider-authored examples;
   - cross-field soundness relationships;
   - request-to-response construction relationships; and
   - uncertainties, contradictions, and missing evidence.
3. Treat implementation behavior as evidence, not permission to publish private details.
4. Do not invent endpoints, statuses, fields, constraints, examples, error codes, or guarantees.
   Report unresolved facts explicitly.

Contract-neutral requirements

- Preserve existing public methods, paths, field names, payload alternatives, and behavior.
- Give every operation a stable, unique ID when HCL, examples, or tooling needs to target it.
- Define every documented success and error response.
- Distinguish request-only and response-only fields where the selected language supports it.
- Encode requiredness, optionality, explicit null, and absence distinctly.
- Use finite literals or enums for closed public value sets.
- Keep exact provider-authored examples as literal witnesses and validate them against their
  operation edge.
- Express mutually exclusive payload alternatives as genuine alternatives, not one object with
  unrelated optional fields.
- Reject unknown object properties only when the real service rejects them.
- Put machine-readable semantics in types, traits, schemas, examples, and semantic relationships,
  not only in prose.
- Attach a cross-field relationship to the narrowest object that owns every participating value.
- Every relationship must be both validatable and constructible during synthesis.
- Use only public request and response paths in request-to-response relationships.
- If a public invariant cannot be expressed faithfully, document the gap instead of approximating
  it.

OpenAPI standard

When Contract format is `openapi`:

- Preserve the existing supported OpenAPI 3.x version. For a new document, prefer OpenAPI 3.1.1
  unless evidence requires another Simile-supported version.
- Use reusable components for shared payloads, parameters, errors, headers, and responses.
- Use standard OpenAPI keywords for field-local validation, including required, type unions or
  nullability, enum, const, format, bounds, array limits, uniqueItems, oneOf, readOnly, and
  writeOnly.
- Use realistic named OpenAPI examples for important successes and errors.
- Use `x-simile-constraints` only for public relationships involving several values.
- Use `x-simile-from-request` on a response media type for public request-to-response bindings,
  conditions, omissions, defaults, and derived values.
- Bind body values with JSON Pointers and bind path, query, header, or cookie values by their public
  parameter names. Use `concat`, `substring`, `mask`, and `coalesce` for public string derivations
  that the operation promises.
- Declare response headers with schemas or public examples. A redirect must declare its `Location`
  header, and binary responses must declare their actual media type and string/binary schema.
- Ensure every JSON Pointer is RFC 6901 and relative to the object or operation edge carrying it.
- Do not place private rules, database details, or provider infrastructure in extensions.

Smithy standard

When Contract format is `smithy`:

- Produce one standalone Smithy 2.0 IDL document with exactly one service as the public root.
- Model the HTTP surface with `@http`; use operation names as stable operation IDs.
- Bind path, query, header, cookie, and payload members with Smithy's HTTP binding traits.
- Mark body members with `@httpPayload`; declare the local `contentType` trait when needed.
- Attach errors to operations and give each error shape `@error` and `@httpError`.
- Use ordinary Smithy shapes and traits for structures, unions, mixins, required members, patterns,
  lengths, ranges, uniqueness, documentation, and read-only values.
- Declare every custom Simile trait locally. Supported public traits include `simileConstraints`,
  `responseRelations`, `simileSchema`, `httpSuccessResponses`, `dataExamples`, `contentType`,
  `untagged`, `collectionVersion`, and `availability`.
- Use `simileConstraints` only for public cross-field relationships.
- Use `responseRelations` on operations for request-to-response construction by status and content
  type.
- Use `simileSchema` only when the public schema cannot be represented by the base Smithy shape
  algebra. Do not use it to embed or refer to another contract format.
- The published Smithy model must compile without consulting OpenAPI or an external Smithy model
  package.

Semantic TypeScript standard

When Contract format is `typescript`:

- Produce declarations only. Do not emit JavaScript, executable initializers, decorators, or
  runtime validators.
- Publish literal `ServiceName` and `CollectionVersion` declarations.
- Define operation interfaces whose names end in `Operations`. Every operation member must declare
  literal method and path values, request content when present, and every response alternative.
- Group parameters under literal `path`, `query`, `header`, and `cookie` object types.
- Model public JSON values directly with object types, interfaces, literals, unions,
  intersections, tuples, arrays, templates, and named references.
- Use `Closed<T>` only for objects that reject undeclared properties.
- Use optional properties for absence and explicit `null` in the value type for nullability; do
  not conflate them.
- Use literal unions and template literal types for finite and patterned scalar contracts. Report
  integer, numeric-bound, string-length, or array-length facets that the current frontend cannot
  express; do not pretend a branded alias enforces them.
- Publish cross-field propositions in `SchemaSoundness`. Supported relationships are `Ordered`,
  `Bounded`, `Implies`, `EqualFields`, `DistinctValues`, `SetRelationship`, `Equation`,
  `Aggregate`, `Template`, and `Lifecycle`, with their supported predicates and expressions.
- Publish request-to-response construction in `ResponseRelations` or resource-local interfaces
  ending in `ResponseRelations`.
- Publish exact literal witnesses in `OperationExamples`.
- A split package must have a small public entrypoint and only relative declaration imports.
- Do not publish an AST, source digest, metadata type, or vocabulary from another contract format
  as the semantic TypeScript contract.

Simile HCL requirements

HCL is optional and additive. Do not duplicate routes, payload schemas, or response definitions
already owned by the selected contract.

Use HCL only for public observable behavior that the contract cannot express adequately, including:

- selecting a declared response status;
- exact named outcomes whose bodies validate against the selected response;
- reusable public error families;
- public request predicates;
- opaque test scenarios;
- protocol-error mappings; and
- declared effects, asynchronous work, and portable dependencies.

Never reproduce private production decision logic. If a private decision can lead to a public
result, expose an opaque scenario that selects that result:

```hcl
scenario "payment_declined" {
  select "createPayment" {
    outcome = "card_declined"
  }
}
```

Only use public request values in predicates. Name dependencies by portable capabilities such as
`db`, `cache`, `queue`, `mail`, `object_store`, `event_bus`, `scheduler`, or `http`. Do not publish
vendor names, credentials, private tables, thresholds, or topology.

Service configuration

Produce one composition-root configuration:

```hcl
service "[SERVICE_NAME]" {
  listen {
    host = "[HOST]"
    port = [PORT]
  }

  [openapi | smithy | typescript] {
    path = "./[CONTRACT_FILE]"
  }

  behaviors = [
    "./behaviors.hcl",
  ]
}
```

Rules:

- The file contains exactly one `service` block.
- The service label matches the CLI service name.
- The selected contract block uses exactly one of `path` or `url`.
- Omit `behaviors` when no behavior module is justified.
- Relative paths resolve from the local configuration directory.
- A remote configuration uses a contract URL and inline behavior declarations.
- A remote TypeScript contract is one self-contained declaration document; split TypeScript
  packages require a local path.
- Default to `127.0.0.1` unless public binding is explicitly required.
- Keep configuration concerned with composition, not endpoint definitions.

Required deliverables

1. A short contract audit: evidence inspected, contradictions, unresolved questions, and assumptions
   deliberately avoided.
2. The completed contract in the selected format.
3. The minimal Simile service configuration.
4. Optional behavior HCL only where justified.
5. An operation coverage table showing requests, success outcomes, error outcomes, soundness
   relationships, response construction, examples, HCL behavior, and remaining synthesized
   outcomes.
6. A validation report with exact commands and results.

Validation

- Parse and compile the selected contract with Simile.
- Treat every Simile compilation error as a contract defect; do not remove a true public rule only
  to make compilation pass.
- Compile any native source checker available for the selected format, such as Smithy tooling or
  `tsc --noEmit`, without treating it as a substitute for Simile compilation.
- Register or compile the service configuration.
- Run representative operations with synthesized requests.
- Run representative operations with supplied valid requests.
- Test malformed and schema-invalid requests.
- Exercise provider-authored success and error examples.
- Validate every synthesized payload against the same compiled contract and semantic constraints.
- Confirm generated responses match exactly one declared response alternative.
- Confirm important errors use provider-authored examples or outcomes rather than random strings.
- Confirm no private implementation details appear in the contract, HCL, examples, metadata, or
  scenario names.
- Report exact commands, results, and any validation that could not be performed.

Quality standard

The finished contract must be useful without HCL, faithful without exposing internals, economical
for the provider to maintain, and strict enough that a successful Simile test gives an integrator
meaningful confidence—not merely schema-shaped random data.
````

Read [Contract sources and HCL](/contract-sources-and-hcl) for the ownership model, then use the
format-specific standards for [OpenAPI](/openapi-and-hcl), [Smithy](/smithy-contracts), or
[semantic TypeScript](/typescript-contracts).
