---
title: CLI reference
description: Find every Simile command, its role, and the workflow guide that explains it.
---

Run `simile --help` to see the command list and `simile COMMAND --help` for every accepted
argument. This page maps those commands to the task they perform.

## Global options

Global options may appear before or after a command.

| Option | Values | Purpose |
| --- | --- | --- |
| `--output` | `human`, `json` | Select terminal-oriented or stable machine-readable output. |
| `--color` | `auto`, `always`, `never` | Control ANSI color in human output. |
| `-v`, `--verbose` | repeatable | Show more execution and provenance detail. |
| `-q`, `--quiet` | flag | Suppress non-essential diagnostics. |
| `--help` | flag | Show help for the current command. |
| `--version` | flag | Show the Simile version. |

Machine-readable output never contains ANSI escapes. Command results go to standard output;
diagnostics and progress go to standard error.

## Environment and definition

| Command | Purpose | Guide |
| --- | --- | --- |
| `simile version` | Print detailed build information. | [Downloads](/downloads) |
| `simile doctor` | Check the local environment and explain actionable problems. | [Quickstart](/quickstart) |
| `simile compile CONTRACT` | Compile one OpenAPI, Smithy, or semantic TypeScript contract, with optional HCL modules, into a `.similec` artifact. | [Contract sources and HCL](/contract-sources-and-hcl) |
| `simile inspect ARTIFACT` | Verify and summarize a compiled artifact. | [Compiled artifact](/compiled-artifact) |
| `simile describe` or `simile describe service [service]` | Explain the current `Simfile`, a registered service, or an explicit configuration. | [Describe a service](/describe-service) |
| `simile audit ARTIFACT` | Explain remaining fidelity gaps and optionally fail on warnings. | [Fidelity audit](/fidelity-audit) |
| `simile playground` | Open the self-contained browser workspace for authoring and exercising a service. | [Playground](/playground) |

## Execute and reproduce operations

| Command | Purpose | Guide |
| --- | --- | --- |
| `simile run CONTRACT` | Compile and execute one operation without starting a server. | [Run one operation](/run-operation) |
| `simile replay BUNDLE` | Re-run a captured execution against its exact artifact and seed. | [Reproduction bundles](/reproduction-bundles) |
| `simile conformance BUNDLE_OR_SUITE…` | Replay provider examples as a deterministic compatibility gate. | [Conformance suites](/conformance-suites) |

`run` accepts exactly one contract source and one operation selector. Use `--operation-id ID` for
the contract token or `--operation "METHOD /path"` for the structural key. `--request` accepts
inline JSON, a JSON file path, or `-` for standard input. When omitted, Simile constructs and
validates a request from the contract.

## Manage services

| Command | Purpose | Guide |
| --- | --- | --- |
| `simile register [service]` | Compile a `Simfile` and install an immutable local revision. | [Lifecycle](/service-lifecycle) |
| `simile update SERVICE` | Reload the registered sources and atomically install a new revision. | [Lifecycle](/service-lifecycle) |
| `simile start [service]` | Start a long-running HTTP emulator from a registration or local `Simfile`. | [Lifecycle](/service-lifecycle) |
| `simile test [service] -- COMMAND…` | Start the emulator, run an application test process, and return its exit status. | [Local testing](/local-testing) |
| `simile state SERVICE` | Inspect persisted resources and causal history. | [State inspection](/state-inspection) |
| `simile remove SERVICE --confirm SERVICE` | Permanently remove one registration and its local service state. | [Lifecycle](/service-lifecycle) |

Set `SIMILE_HOME` to isolate the registry, profiles, reports, and persisted state for a developer,
test job, or container.

## Manage caller profiles

| Command | Purpose | Guide |
| --- | --- | --- |
| `simile setup SERVICE` | Run provider-declared setup and save one concrete caller profile. | [Set up a service](/service-setup) |
| `simile profiles SERVICE` | List the profiles created for a service. | [Service profiles](/service-profiles) |
| `simile profile SERVICE PROFILE` | Inspect one profile without revealing credentials. | [Service profiles](/service-profiles) |
| `simile use SERVICE --profile NAME` | Select the default profile for CLI-originated work. | [Service profiles](/service-profiles) |

## Learn and report

| Command | Purpose | Guide |
| --- | --- | --- |
| `simile proxy SERVICE --to URL` | Forward local application calls to a real provider and optionally learn a draft service. | [Proxy and learn](/proxy-and-learn) |
| `simile report TARGET` | Prepare, inspect, verify, or send one reproducible emulator issue. | [Report an issue](/issue-reporting) |
| `simile reports [service]` | List locally prepared reports. | [Report an issue](/issue-reporting) |
| `simile reporting SERVICE` | Inspect or change automatic reporting consent for a profile. | [Report an issue](/issue-reporting) |

## Exit and output behavior

Simile returns a nonzero status for invalid input, compilation failure, failed verification, or a
test command that failed. `simile test` returns the child command's status. A closed output pipe is
treated as a normal terminal condition, which keeps commands safe to use with tools such as
`head`, `jq`, and pagers.
