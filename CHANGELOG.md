# Changelog

All notable changes to Simile will be documented in this file.

## Unreleased

- Add automatic redb-backed Stack component checkpoints under `SIMILE_HOME`, with restart recovery,
  exclusive ownership, format and resource compatibility checks, and rollback when persistence
  fails.
- Remove the unused `behavior.trigger` HCL form and operation-to-async artifact links. Async
  declarations remain available for service discovery until the effect model owns attachment and
  dispatch.
- Advance the compiled artifact format to version 13. Existing artifacts must be recompiled.

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
