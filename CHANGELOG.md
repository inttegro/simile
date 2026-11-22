# Changelog

All notable changes to Simile will be documented in this file.

## Unreleased

- Adopt `Simfile` as the conventional local service definition so `register`, `describe`, and
  `start` can discover and name a service from the current directory.

- Treat request-to-response bindings as hard postconditions across body, path, query, header, and
  cookie sources, with explicit failures when a reachable response cannot be constructed.
- Add request-derived concatenation, substring, masking, and conditional-default expressions.
- Make response synthesis minimal by default so unrelated optional properties and union branches
  stay absent.
- Encode declared JSON, text, form URL-encoded, multipart, and octet-stream responses.
- Compile and synthesize declared response headers, including bodyless redirects with `Location`.
- Add provider-authored state projections that resolve stored public references into a private
  response-synthesis view without changing the validated caller request.
- Let state projections select declared public outcomes for missing references and failed public
  predicates while retaining provider-facing errors for undeclared failures.
- Complete schema-required response fields around stored mutation snapshots before applying the
  operation's published request-to-response relations.
- Advance the compiled artifact format to version 24. Existing artifacts must be recompiled.

## 0.3.0 - 2026-11-20

- Add provider-authored principal contracts, principal-setup workflows, and durable named profiles,
  including interactive provisioning, private credential exports, dynamic request authentication,
  credential-scoped OAuth grants,
  profile-scoped webhook destinations and signing, profile inspection, CLI selection, and
  collision-safe human-readable profile names when `--profile` is omitted.
- Rename the public HCL `resource` block to `backend`, including authoring diagnostics,
  descriptions, command output, and documentation. Compiled Stack resource addresses remain an
  internal runtime detail.
- Add first-class Smithy 2.0 and declarations-only semantic TypeScript contract frontends for
  `compile`, `run`, and service configurations, including split TypeScript package resolution and
  lowering of public soundness and request-to-response relations into the canonical artifact.
- Add automatic redb-backed Stack component checkpoints under `SIMILE_HOME`, with normalized
  metadata, resource, replay, journal, counter, and clock tables; restart recovery; v1 migration;
  exclusive ownership; compatibility checks; and rollback when persistence fails.
- Add typed HCL effects, synchronous transactional Stack actions, post-delivery async dispatch,
  conservative database-write inference, and a protocol-neutral service supervisor.
- Compile and validate OpenAPI path, query, header, and cookie parameters, including path-item
  inheritance and operation-level overrides.
- Add seeded reproduction bundles, exact replay, batch conformance suites, explainable fidelity
  audits, and compatibility metadata.
- Add durable state and causality inspection with resource, sequence, causation, and correlation
  filters; stored values require explicit opt-in.
- Bind durable state to service and artifact identity, prevent removal while state is owned by a
  running process, and remove the state database with a confirmed service deletion.
- Remove the unused `behavior.trigger` HCL form; effects now own operation attachment and async
  dispatch.
- Add a service-wide webhook infrastructure policy with best-effort ordering by default, bounded
  probabilistic reordering, hard dependency protection, and reproducible runtime decisions.
- Import OpenAPI callbacks and OpenAPI 3.1/3.2 top-level webhooks, allow provider-authored fallback
  contracts, and compile compact multi-webhook effect emissions into schema-checked occurrences.
- Advance the compiled artifact format to version 20. Existing artifacts must be recompiled.

## 0.2.0 - 2026-11-18

- Prove request-aware response synthesis with provider-authored request-to-response bindings,
  OpenAPI patterns, lifecycle reachability, and response soundness checks.
- Compile reusable environmental fault catalogs, namespace selectors and exclusions, deterministic
  profiles, and bounded delay or disconnect transport plans.
- Add exact-confirmation service removal and expand the service registry lifecycle.
- Advance the compiled artifact format to version 11. Existing artifacts must be recompiled.
- Add the Simile discovery site, installation downloads, architecture references, OpenAPI extension
  guides, service-authoring guidance, and a fault glossary.

## 0.1.1 - 2026-11-17

- Add a stable shell-install bootstrap for macOS and Linux.
- Publish Apple Silicon release artifacts on GitHub's supported macOS 15 runner.

## 0.1.0 - 2026-11-17

- Establish the Rust workspace, command architecture, tests, CI and release configuration.
- Execute operations directly from local or hosted OpenAPI contracts, with structural or
  `operationId` selection, schema-valid request synthesis for declared media types, and a
  response-only default output suitable for pipelines.
- Generate varied schema-valid request and response values, with seeded reproduction and
  in-memory replay for idempotency keys and GET resource identities.
- Compile ordered numeric and RFC 3339 date-time relationships from `x-simile-constraints`, enforce
  them for incoming payloads, and preserve them through sound request and response synthesis.
- Compile conditional implications with conjunctive consequences, prove them for incoming payloads,
  support membership and path-comparison antecedents, and establish triggered consequences to a
  bounded fixed point during synthesis.
- Prove and construct equality across array items, numeric equations, guarded collection sums,
  derived string templates, and acyclic finite lifecycle snapshots from typed
  `x-simile-constraints` declarations.
- Prove and construct relative numeric and date-time bounds, distinct selected values, scalar set
  relationships, literal and multiplicative equations, and count, minimum, and maximum aggregates.
- Apply ordinary soundness constraints recursively to array item schemas so each generated or
  validated item obeys its own local relationships without a separate iteration language.
