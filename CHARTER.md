# Simile service charter

## Mission

Simile enables service providers to distribute a faithful, locally runnable model of their
systems. A developer can exercise the service's API, state transitions, and asynchronous behaviour
on a laptop or in CI without depending on a provider-operated test environment.

Simile is a portable test mode: the provider defines what the service does, Simile executes that
definition, and the developer owns the local runtime.

## The problem

An integration needs more than plausible HTTP responses. Real operations often validate existing
state, create new records, enqueue work, emit events, call webhooks, retry failures, or settle into
a final state later. Static mocks and fixture servers hide this behaviour, while hosted test modes
require the provider to operate and support another multi-tenant system.

Simile gives providers a third option. They can publish the behaviour of their service as a
versioned definition and let each developer run that behaviour locally. This shifts test-mode
execution and availability to the developer without shifting ownership of the service's semantics.

## Operating model

The service provider defines:

- the externally visible HTTP contract in a published OpenAPI document;
- request validation, responses, errors, and state transitions;
- optional HCL behaviour attached to operations by their published `operationId`;
- asynchronous work such as events, webhooks, queues, retries, and delayed outcomes;
- supported scenarios, including deliberate failures and edge cases; and
- the relationship between a definition version and the corresponding service contract.

Simile provides:

- a consistent local runtime for those definitions;
- isolated, disposable state that can be inspected and reset;
- deterministic execution and explicit control over asynchronous progress;
- observable operation, event, and failure history;
- stable human and machine interfaces for local development and CI; and
- clear reporting when behaviour is unsupported or intentionally differs from production.

The developer chooses a service definition, supplies local configuration and data, and runs the
emulator in an environment they control. Their application talks to Simile as it would talk to the
service, while Simile performs the provider-defined effects locally.

## Definition model

Every emulatable service publishes an OpenAPI document. OpenAPI owns the method, path, request,
response, authentication, and error contract. Simile accepts the OpenAPI 3.0, 3.1, and 3.2 feature
lines and preserves the provider's exact patch version.

HCL is an optional behavior overlay. An operation without HCL remains executable from OpenAPI:
Simile validates the request, prefers published examples, and otherwise synthesizes a
schema-valid deterministic response. It does not infer storage, CRUD semantics, or asynchronous
work from a route name.

An HCL `behavior` may select a response already declared by OpenAPI and attach observable work that
OpenAPI cannot describe. It cannot add an endpoint, contradict a schema, or name a private
production check. HCL may also declare non-HTTP async operations and the abstract resources they
use.

The supported resource vocabulary describes Simile's local emulation substrate: `db`, `cache`,
`queue`, `mail`, `object_store`, `event_bus`, `scheduler`, and `http`. It deliberately excludes
products such as PostgreSQL, Redis, or SES. Production topology is not part of the portable
behavioral contract.

## Product principles

### Behavioural, not static

An endpoint is not complete when it returns the expected status and body. Its state changes and
downstream effects are part of the contract. Simile models the operation, not merely its response.

### Asynchrony is first-class

Background work must be definable, observable, and controllable. Tests should be able to wait for,
advance, inspect, and reproduce asynchronous outcomes without arbitrary sleeps.

### Local-first and self-contained

Once a definition and its declared dependencies are available, normal emulation should not depend
on the provider's availability. The same definition should run on a developer machine and in CI.

### Provider-authored and versioned

The provider remains the authority on service semantics. Definitions are explicit, reviewable, and
versioned so a test run can be reproduced against the same behavioural contract.

### Deterministic by default

Time, randomness, retries, identifiers, and failure scenarios must be controllable where they
affect reproducibility. Simile should make nondeterminism deliberate rather than incidental.

### Inspectable, disposable state

Developers must be able to understand what happened, inspect current state, and return to a known
starting point. Resetting an emulator should be cheaper and safer than repairing a shared test
account.

### Honest fidelity

Simile must distinguish implemented behaviour from approximation or omission. It should never
imply production equivalence where the definition does not provide it.

### Public behavior, not private implementation

A distributed definition is inspectable. It may describe externally observable results and safe
scenario controls, but it must not reveal private fraud rules, security thresholds, internal
algorithms, credentials, or production topology. Sensitive production decisions are represented
by provider-authored outcomes, never shipped logic.

### Safe to run

Definitions and runtime behaviour must not silently expose credentials, contact production systems,
or perform undeclared network activity. External effects require explicit configuration and must be
visible to the developer.

## Responsibility boundary

The service provider owns the accuracy and maintenance of its definition. Simile owns the runtime,
lifecycle, isolation, observability, and execution semantics. The developer owns the environment in
which the emulator runs and the local data and configuration supplied to it.

Moving test mode onto the developer's infrastructure does not move the meaning of the service onto
the developer. A provider cannot call a definition faithful unless it maintains that definition as
its production contract evolves.

## Non-goals

Simile is not:

- a production service or a production failover mechanism;
- a static fixture server with programmable response matching;
- a transparent traffic proxy or record-and-replay tool;
- a promise to reproduce production scale, latency, infrastructure, or undocumented behaviour;
- a provider-hosted shared sandbox; or
- a substitute for provider conformance tests against the real service.

Those tools can complement Simile, but they do not define it.

## Measure of success

Simile succeeds when a developer can run a meaningful integration end to end, including its
asynchronous consequences, entirely in an environment they control; reproduce the same scenario in
CI; understand why it behaved as it did; and move to the real service without learning a different
contract.

It succeeds for a service provider when maintaining a versioned behavioural definition is more
reliable and economical than operating a separate hosted test-mode platform for every developer.
