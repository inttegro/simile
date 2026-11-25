---
title: Glossary
description: Definitions for the language, compilation model, and runtime concepts used by Simile HCL.
---

This glossary gives one stable meaning to terms used across the Simile HCL reference and guides.
Definitions describe Simile v0.5.1. Follow each link for syntax or execution details.

## A

### Async operation

A named declaration created with [`async`](/hcl-language-specification#async). It lists the
backends that background work may use. An effect attaches it to contract operations; the service
supervisor dispatches its actions outside the immediate response path.

## B

### Backend

A provider-neutral backing system named by a [`backend`](/hcl-language-specification#backend)
block, such as `db`, `queue`, or `mail`. It gives effects and asynchronous work a local capability
while keeping production vendor and topology details private.

### Backend reference

The canonical `KIND.NAME` identifier for a declared backend, such as `queue.orders`.

### Behavior

An HCL overlay for one exact contract operation ID. A behavior may prefer a response status and
attach named outcomes or reusable error sets. It cannot add or redefine an HTTP operation.

### HCL module

An HCL document containing service-model declarations but no `service` or `principal_setup` block.
Modules imported by `service.modules` and declarations inline in the service configuration share
one compiled namespace. A module may define principals, profiles, operation behavior, effects,
scenarios, faults, asynchronous work, or webhooks.

### Binding

An executable relationship that relates a response field to request data. OpenAPI authors use
`x-simile-from-request`; Smithy and semantic TypeScript provide native source forms that lower to
the same compiled relation. Bindings belong to the contract rather than HCL. See
[request–response relations](/request-response-relations).

## C

### Catalog fault

A complete, provider-neutral environmental fault included with Simile and imported through
`faults { use "..." {} }`. Compilation copies its definition into the service artifact.

### Compilation

The phase that merges parsed HCL modules with the selected contract source, resolves names, checks cross-source
invariants, validates exact outcome bodies, and produces a deterministic service model.

### Compiled service

The normalized in-memory result of compilation: contract operations plus resolved behavior,
outcomes, scenarios, faults, backends, and async declarations.

### Configuration

The HCL composition root containing exactly one `service` block, its listener, one contract source,
and optional inline or imported module declarations.

### Constraint

An executable cross-field relationship on a contract object. OpenAPI authors use
`x-simile-constraints`; Smithy and semantic TypeScript provide native source forms that lower to
the same compiled constraints. Constraints prove relationships within a payload and are distinct
from HCL behavior. See [payload soundness](/payload-soundness).

### Credential

A credential slot on a principal corresponds to one contract security scheme and carries any
granted scopes. A profile fills the slot with a synthetic secret or HTTP Basic pair. Simile omits
concrete values from descriptions and audit output. Local and CI profiles must never reuse
production secrets.

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

### Durable outbox

The persistent queue of asynchronous actions and webhook deliveries committed together with an
operation's synchronous state. Workers lease items from the outbox, acknowledge completed work,
schedule retries, and move exhausted items to dead letters. See [Durable work](/durable-work).

## E

### Effect

A provider-authored attachment connecting committed operations to synchronous Stack actions,
named asynchronous work, and/or webhook occurrences. Effects are deliberately separate from
`behavior`, which owns HTTP result selection.

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

### Hard dependency

An explicit causal edge requiring one webhook occurrence to follow another. A hard dependency is
never weakened by best-effort infrastructure or a stochastic reordering decision.

## L

### Literal value

A value that can be evaluated without variables or external functions and converted to JSON.
`outcome.body`, `protocol_error.body`, and `when.equals` accept literal values.

### Lease

Temporary ownership of one durable work item. A worker must acknowledge or release the item before
the visibility timeout; otherwise it becomes eligible for another worker. Leases allow recovery
when a process stops between receiving and completing work.

## N

### Named outcome

A response candidate with a stable public name, HTTP status, optional media type, optional exact
body, and optional request predicates. A scenario or CLI invocation can select it by name.

## O

### Contract authority

The rule that the selected OpenAPI, Smithy, or semantic TypeScript source owns the HTTP contract.
HCL references and narrows that contract but cannot invent routes, schemas, or response statuses.

### Operation ID

The exact stable operation identifier declared by the contract and used by HCL `behavior`,
scenario `select` and `inject`, and fault-profile operation scopes. It is not the `METHOD /path`
operation key.

### Outcome provenance

The origin recorded for a response candidate: contract-derived, provider-authored,
recorded-and-reviewed, or schema-synthesized. The serialized `open_api_derived` name is retained for
artifact compatibility and applies to all contract frontends. Provenance indicates how much
authority the body has; it does not by itself prove request-to-response reachability.

## P

### Predicate

A public request condition declared with `when`. Its `path` is a JSON Pointer into the validated
request body and its `equals` value must match exactly. Every predicate on an outcome must match.

### Principal

A named kind of service caller declared in HCL. A principal defines the credential slots and grants
shared by all of its profiles. Its labels reference security schemes published by the contract.

### Principal setup

The provisioning workflow declared by a top-level `principal_setup` block. It collects caller
configuration, runs one contract operation, issues identifiers or credentials, and creates or
updates a named profile of the referenced principal. A service may declare one workflow for each
principal kind it knows how to provision.

### Profile

One concrete instance of a principal. A durable setup-created profile groups collected inputs,
issued identifiers and credentials, a webhook destination, and a webhook signing secret. HCL may
also declare fixture profiles for deterministic authentication tests. Every active profile may
authenticate concurrently; the current profile supplies a CLI default.

### Protocol error

A failure discovered at the HTTP/runtime boundary before normal operation execution, mapped through
a provider-neutral cause such as `invalid_body` or `unsupported_media_type` into a response the
contract operation declares.

## R

### Reachability

The property that a generated response is a possible result of the submitted request and operation.
Request–response bindings, constraints, outcome predicates, and lifecycle rules establish this
stronger guarantee beyond isolated schema validity.

### Replay identity

An idempotency key or stable GET resource identity used to return the same completed result within
one running service process. Durable backend state has a separate lifecycle.

### Report

A local record of one verified emulator issue. It contains technical metadata, a verification
fingerprint, and a self-contained reproduction case. Reporter contact travels separately during
submission.

### Reporting destination

The HTTPS receiver published by `service.reporting.endpoint`. The declaration exposes a provider
capability. Permission to send remains with the user and is stored per profile or granted for one
manual submission.

### Reporting preference

Profile-scoped user consent for automatic issue reporting, together with a private contact address
and the exact destination shown when consent was granted.

### Reorder window

The maximum scheduled-time separation within which two independent webhook deliveries are
eligible to overtake one another. The window prevents distant occurrences from being arbitrarily
shuffled.

## S

### Scenario

An opaque, named deterministic test control. It selects public outcomes and/or injects environmental
faults for specific contract operation IDs without exposing the provider's private decision tree.

### Schema-valid

Conforming to one compiled contract schema in isolation. Schema validity is necessary but weaker than
payload soundness or operation reachability.

### Security requirement

The contract rule describing which security schemes protect an operation. Simile preserves
OpenAPI's composition: schemes within one requirement are conjunctive, while multiple requirements
are alternatives. An empty requirement marks an operation public.

### Service artifact

The versioned `.similec` CBOR envelope containing the deterministic compiled service. It is the
runtime authority for a registered service.

### Soundness

The property that a payload satisfies both its field schemas and declared relationships among
fields. Soundness clauses live in the selected contract source; HCL selects observable behavior.

### Stochastic fault

An environmental fault selected by a `fault_profile` according to a bounded probability policy.
Stochastic selection never chooses domain outcomes or parameter errors.

## V

### Validated request

A request body successfully decoded for the selected media type and accepted by the operation's
compiled contract request schema. HCL outcome predicates evaluate only against this body.

## W

### Webhook contract

The public wire description for one outbound HTTP message: its stable name, method, optional
request-body schema, and optional callback target expression. Contracts normally come from
OpenAPI callbacks or top-level webhooks; a singular HCL `webhook` block can fill a documented gap.

### Webhook occurrence

One rendered outbound message caused by a committed effect. An occurrence has a payload and may
have an explicit destination, delay, and hard dependencies on sibling occurrences. It is distinct
from the reusable webhook contract.

### Webhook policy

The service-wide `webhooks` configuration governing delivery ordering, bounded reordering
probability, concurrency, and optional HMAC-SHA256 signing. Webhook contracts describe payloads,
while effects decide which occurrences are emitted.

Read the [language specification](/hcl-language-specification) for the complete syntax and
compile-time rules.
