---
title: Glossary
description: Definitions for the language, compilation model, and runtime concepts used by Simile HCL.
---

This glossary gives one stable meaning to terms used across the Simile HCL reference and guides.
Definitions describe Simile v0.2.0. Follow each link for syntax or execution details.

## A

### Abstract resource

A provider-neutral capability named by a [`resource`](/hcl-language-specification#resource) block,
such as `db`, `queue`, or `mail`. It describes what emulated behavior needs without publishing a
production vendor or topology.

### Async operation

A named declaration created with [`async`](/hcl-language-specification#async). It lists the abstract
resources that background work may use. Simile currently compiles and describes the declaration but
does not attach it to an HTTP operation or execute it.

## B

### Behavior

An HCL overlay for one exact OpenAPI `operationId`. A behavior may prefer a response status and
attach named outcomes or reusable error sets. It cannot add or redefine an HTTP operation.

### Behavior module

An HCL document containing behavior declarations but no `service` block. Modules imported by
`service.behaviors` and declarations inline in the service configuration share one compiled
namespace.

### Binding

An executable OpenAPI extension that relates a response field to request data, such as
`x-simile-from-request`. Bindings belong to the OpenAPI schema rather than HCL. See
[request–response relations](/request-response-relations).

## C

### Catalog fault

A complete, provider-neutral environmental fault included with Simile and imported through
`faults { use "..." {} }`. Compilation copies its definition into the service artifact.

### Compilation

The phase that merges parsed HCL modules with OpenAPI, resolves names, checks cross-source
invariants, validates exact outcome bodies, and produces a deterministic service model.

### Compiled service

The normalized in-memory result of compilation: OpenAPI operations plus resolved behavior,
outcomes, scenarios, faults, abstract resources, and async declarations.

### Configuration

The HCL composition root containing exactly one `service` block, its listener, one OpenAPI source,
and optional inline or imported behavior declarations.

### Constraint

An executable `x-simile-constraints` clause on an OpenAPI schema. Constraints prove relationships
within a payload; they are distinct from HCL behavior. See the
[constraint reference](/x-simile-constraints).

## D

### Description

Optional public presentation metadata on a named semantic HCL object. It is retained in the
compiled artifact for `simile describe service` and never changes runtime behavior. Structural
blocks such as `listen`, `when`, and `select` do not have descriptions.

### Deterministic fault

An environmental fault selected explicitly by a scenario. Given the same scenario and operation,
the same fault is chosen; no probability is involved.

### Domain outcome

A service-level result such as `card_declined`, `requires_payment`, or `manual_review`. Domain
outcomes are selected by a public request predicate, an explicit outcome name, or an opaque
scenario—not by stochastic fault injection.

## E

### Effect

A future provider-authored attachment that will connect a completed operation to state mutation or
background work. Effects are deliberately separate from `behavior`, which owns HTTP result
selection. Effect syntax and execution are not part of the current language version.

### Environmental fault

A disruption to the delivery of a completed service result, such as delay or a connection lost
after commit. It is separate from the domain outcome and does not make valid input randomly invalid.

### Error set

A named, reusable collection of provider-authored outcomes. It becomes available to operations
through `behavior.error_sets` or `apply_error_set`.

## F

### Fault action

The concrete disruption performed by a fault. The current actions are `delay` and `disconnect`.

### Fault phase

The point where an environmental fault acts: `before_response` or `after_commit`.

### Fault profile

A bounded stochastic policy that selects only named environmental faults. It defines probability,
operation scope, repetition limits, cooldown, and an optional seed.

## H

### HCL

HashiCorp Configuration Language. Simile uses HCL's native notation as the concrete syntax for its
own small domain-specific language; it does not implement Terraform semantics.

## L

### Literal value

A value that can be evaluated without variables or external functions and converted to JSON.
`outcome.body`, `protocol_error.body`, and `when.equals` accept literal values.

## N

### Named outcome

A response candidate with a stable public name, HTTP status, optional media type, optional exact
body, and optional request predicates. A scenario or CLI invocation can select it by name.

## O

### OpenAPI authority

The rule that OpenAPI owns the HTTP contract. HCL references and narrows that contract but cannot
invent routes, schemas, or response statuses.

### Operation ID

The exact OpenAPI `operationId` used by HCL `behavior`, scenario `select` and `inject`, and fault
profile operation scopes. It is not the `METHOD /path` operation key.

### Outcome provenance

The origin recorded for a response candidate: OpenAPI-derived, provider-authored,
recorded-and-reviewed, or schema-synthesized. Provenance indicates how much authority the body has;
it does not by itself prove request-to-response reachability.

## P

### Predicate

A public request condition declared with `when`. Its `path` is a JSON Pointer into the validated
request body and its `equals` value must match exactly. Every predicate on an outcome must match.

### Protocol error

A failure discovered at the HTTP/runtime boundary before normal operation execution, mapped through
a provider-neutral cause such as `invalid_body` or `unsupported_media_type` into a response the
OpenAPI operation declares.

## R

### Reachability

The property that a generated response is a possible result of the submitted request and operation,
not merely valid against an isolated response schema. Request–response bindings, constraints,
outcome predicates, and lifecycle rules contribute to reachability.

### Replay identity

An idempotency key or stable GET resource identity used to return the same completed result within
one running service process. Replay is not a durable resource database.

### Resource reference

The canonical `KIND.NAME` identifier for an abstract resource, such as `queue.orders`.

## S

### Scenario

An opaque, named deterministic test control. It selects public outcomes and/or injects environmental
faults for specific OpenAPI operation IDs without exposing the provider's private decision tree.

### Schema-valid

Conforming to one compiled OpenAPI schema in isolation. Schema validity is necessary but weaker than
payload soundness or operation reachability.

### Service artifact

The versioned `.similec` CBOR envelope containing the deterministic compiled service. It is the
runtime authority for a registered service.

### Soundness

The property that a payload satisfies both its field schemas and declared relationships among
fields. Soundness clauses live in OpenAPI `x-simile-constraints`; HCL selects observable behavior.

### Stochastic fault

An environmental fault selected by a `fault_profile` according to a bounded probability policy.
Stochastic selection never chooses domain outcomes or parameter errors.

## V

### Validated request

A request body successfully decoded for the selected media type and accepted by the operation's
compiled OpenAPI request schema. HCL outcome predicates evaluate only against this body.

Read the [language specification](/hcl-language-specification) for the complete syntax and
compile-time rules.
