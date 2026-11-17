---
title: Service lifecycle
description: Understand how register, update, start, and run compile and load service definitions.
---

Simile has two execution paths. `run` compiles and executes one operation in a short-lived process.
`register` and `start` create a reusable local service. They share the compiler but deliberately
have different persistence and replay behavior.

```mermaid
flowchart TD
  Sources[OpenAPI + optional HCL]
  Sources --> Run[simile run]
  Run --> One[Compile in memory\nexecute once\nexit]
  Sources --> Register[simile register]
  Register --> Revision[Immutable registry revision]
  Revision --> Start[simile start NAME]
  Start --> Server[Long-lived local HTTP server]
  Sources --> Update[simile update NAME]
  Update --> NewRevision[New immutable revision]
  NewRevision --> Pointer[Atomically advance current]
```

## Register

```bash
simile register inttegro --config ./inttegro.simile.hcl
```

Registration performs the expensive and failure-prone work before installation:

1. load the service configuration from a local path or HTTP(S) URL;
2. resolve and load its OpenAPI source and behavior modules;
3. parse and semantically validate both languages;
4. compile schemas, outcomes, constraints, and heuristic plans;
5. encode and decode the `.similec` artifact to verify it; and
6. install a private immutable revision, then atomically advance `current`.

If the service already exists, registration fails unless `--force` is explicit. Routine refreshes
should use `update` instead.

## Start

```bash
simile start inttegro
```

Starting a registered service reads the current manifest and artifact from the registry. It does
not reopen the original configuration, read local OpenAPI files, or refetch remote sources. This
makes startup independent of the provider's availability and ensures the process runs the exact
revision that registration validated.

For an ad-hoc local run, bypass the registry:

```bash
simile start inttegro --config ./inttegro.simile.hcl --port 8484
```

`--port` changes only that invocation. `start --config` currently accepts a local path; URL-based
configuration loading belongs to `register` and `update`.

## Update

```bash
simile update inttegro
```

Update reloads the stored configuration snapshot and the sources it names, compiles a complete new
revision, and advances `current` only after success. A failed fetch, parse, semantic check, schema
compile, or artifact check leaves the active revision unchanged.

To replace the configuration source at the same time:

```bash
simile update inttegro --config https://example.com/inttegro.simile.hcl
```

An already running process keeps the artifact it loaded. Restart it to load the newly current
revision.

## Run

```bash
simile run ./openapi.yaml --operation-id createOrder
```

`run` compiles the OpenAPI document and optional `--behavior` modules in memory, executes one
operation, prints the result, and exits. It does not read or write the service registry. Because
each invocation creates a fresh runtime, replay state does not cross CLI invocations.

## Registry layout

The default registry root is `~/.simile`; set `SIMILE_HOME` for CI isolation or tests:

```text
~/.simile/
├── layout-version
├── services/
│   └── inttegro/
│       ├── current
│       ├── revisions/
│       │   └── REVISION/
│       │       ├── config.hcl
│       │       ├── manifest.json
│       │       └── service.similec
│       └── state/
├── locks/
└── tmp/
```

`state/` is reserved outside immutable revisions for future durable emulated state. The current
runtime does not persist resources there. `locks/` is likewise reserved as cross-process registry
coordination expands; installation currently relies on same-filesystem staging and atomic rename.

See [Service configuration](/service-configuration) for configuration rules and
[Compiled artifact](/compiled-artifact) for what the installed runtime reads. The component
ownership and atomic installation flow are described in [Application and registry](/application-registry).
