# Changelog

All notable changes to Simile will be documented in this file.

## Unreleased

## 0.6.0 - 2026-11-27

- Add `simile playground`, a bundled browser workspace for authoring, compiling, running,
  inspecting, and exporting service definitions without a separate frontend installation.
- Add a portable WebAssembly runtime and shared HTTP execution layer so browser workers and native
  listeners use the same request validation, profile authorization, effects, and response rules.
- Add an authenticated browser-console workbench for running compiled operations with profiles,
  scenarios, and fault profiles through the live service pipeline.
- Generate representative requests and responses from compiled operation contracts and preserve
  precise protocol errors when a request fails validation.
- Publish the browser playground source, architecture, parity checks, persistence model, and native
  handoff workflow.

## 0.5.1 - 2026-11-25

- Expand `simile console` into an immersive terminal interface and add live, searchable HTTP and
  Stack logs to the browser console, including split and full-screen views.
- Preserve registered contract sources and compatible durable service state across updates and
  restarts, while deriving request-related timestamps from the service clock.
- Decode documented `scheduler.schedule` effects correctly and retain failed effect commits in
  exchange history so they remain inspectable and reportable.
- Preserve path, query, header, cookie, multipart, and binary request evidence in reproduction
  bundles, including safe deterministic fixtures for file parts.
- Accept current reproduction-bundle versions in the report receiver and verify complete evidence
  without silently discarding transport data.

## 0.5.0 - 2026-11-24

- Add `simile console` with terminal and browser interfaces for operations, outcomes, scenarios,
  faults, profiles, exchanges, Stack state, deferred work, and fidelity evidence.
- Add a loopback-only runtime inspection surface with sanitized exchange capture, local state
  controls, and verified issue-report preparation.
- Persist sanitized exchange history and prepared report state across service restarts, retain it
  until explicit clearing, and remove it with the registered service.
- Restore compatible durable Stack state across artifact updates, initialize newly declared
  backends, and update the persisted artifact identity after a successful checkpoint.
- Drive request-related response timestamps from the durable Stack logical clock while preserving
  explicit request bindings and contract-controlled scheduling fields.
- Compose reusable HCL declarations through `service.modules`; retain `behaviors` as a deprecated
  compatibility alias.
- Expand the reference report receiver with fingerprinted triage, affected-version tracking, and
  isolated verification evidence.
- Refine terminal startup, request-log, and service-description presentation.
- Reorganize the documentation around the user journey from starting and running a service through
  testing, inspection, authoring, learning, and issue reporting.

## 0.4.0 - 2026-11-22

- Add `simile test` for supervised application tests and `simile proxy --learn` for capturing
  sanitized HTTP evidence and producing a conservative OpenAPI contract and Simfile.
- Adopt `Simfile` as the conventional local service definition so `register`, `describe`, and
  `start` can discover and name a service from the current directory.
- Add provider-declared report destinations, profile-owned automatic-reporting consent, one-time
  manual submissions, private contact storage, upgrade preflight, and local report history.
- Capture sanitized internal runtime failures as exact incident reproductions, verify them before
  submission, deduplicate fingerprints, and keep reporting work outside the HTTP request path.

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
- Add a container image and practical guides for local, Docker, CI, and GitHub Actions testing.
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
