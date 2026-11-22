---
title: Test infrastructure costs
description: Compare hosted test environments and local emulation across infrastructure, engineering time, security, and fidelity maintenance.
---

The invoice for compute and managed services captures only part of a test environment's cost.
Teams also pay for provisioning, access administration, developer waiting time, reliability work,
incident response, and the engineering required to keep test behavior representative. This guide
provides a measured way to compare those costs before moving routine integration tests to Simile.
{{ className: 'lead' }}

## Prerequisites

- Choose a representative measurement period, usually one month or one quarter.
- Collect provider invoices, CI usage, environment setup times, access requests, test failure
  causes, and engineering work associated with the current test environment.
- Read [Design trade-offs](/design-trade-offs) and decide which provider-hosted checks must remain.

## Measure the complete cost

Local emulation changes the location and shape of spending. Hosted environments consume provider
resources and administration; local environments consume developer or CI resources and fidelity
maintenance. A useful comparison records both sides with the same scope.

| Cost category | Provider-hosted environment | Local Simile environment |
| --- | --- | --- |
| Compute and services | Provider compute, databases, queues, storage, networking, observability, and data transfer | Developer or CI CPU, memory, disk, caches, artifacts, and runner minutes |
| Idle capacity | Resources may continue running between test sessions | A supervised emulator can start for one test command and stop with it |
| Provisioning | Accounts, tenants, service resources, allowlists, seed data, and teardown automation | Binary or container distribution, contract compilation, profiles, and local setup |
| Identity and secrets | Sandbox users, OAuth grants, CI variables, webhook secrets, rotation, revocation, and audit | Synthetic credentials, local profile protection, and removal of real provider secrets from test jobs |
| Developer time | Network latency, setup queues, rate limits, shared-state conflicts, and provider outages | Startup and compilation time, machine contention, and investigation of fidelity gaps |
| Engineering ownership | Environment automation, lifecycle management, monitoring, and provider integration | Definition authorship, runtime maintenance, conformance suites, and release compatibility |
| Incident response | Potential review and rotation across every system trusted by the environment | Usually bounded to the machine or CI job when external trust and real secrets are excluded |

The last row depends on configuration. A local process that receives production credentials or
runs on a shared internet-accessible host can create a broad incident boundary. The cost advantage
comes from designing local tests with synthetic credentials, private listeners, isolated state, and
disposable jobs.

## Separate direct spend from elapsed time

### Hosted infrastructure can accrue cost while idle

Non-production resources are often sized for availability even when developers use them only during
working hours. AWS Prescriptive Guidance says scheduling an environment for a 50-hour work week
instead of 168 continuous hours can reduce the affected instance cost by up to 70 percent. The same
guide reports that Jamaica Public Service used Instance Scheduler to manage non-production
environments and reduced costs by 40 percent.

Read the
[AWS scheduling guidance and case studies](https://docs.aws.amazon.com/prescriptive-guidance/latest/optimize-costs-microsoft-workloads/windows-ec2-schedules.html)
with their stated scope. The 70 percent figure is an illustrative instance-scheduling case, and the
40 percent figure is a reported customer result. Neither is a general savings estimate for Simile
or for a complete cloud bill. They show that idle non-production capacity can be financially
meaningful even before network, storage, database, and administration costs are included.

AWS makes the local alternative concrete in its own
[AWS Emulator documentation](https://docs.aws.amazon.com/emulator/latest/userguide/what-is-service.html).
AWS says local and CI tests can run without an AWS account or AWS resources and that the emulator
has no additional service fee. The host machine and CI runner still have a cost, and final testing
on real AWS still consumes resources. The economic change is narrower: repeated modeled workflows
no longer require a live provider resource for every execution.

### Provisioning time affects delivery

Environment lead time begins when a developer needs a tenant, permissions, resources, seed data,
or a clean shared state and ends when useful testing can start. The delay can appear in access
queues, automation runs, manual fixes, and coordination with another team. Measuring only the
hands-on portion hides the elapsed time during which work is blocked.

A Google Cloud customer case study reports that LiquidShare reduced the cost of running instances
by around 20 percent after migrating its infrastructure. It also reports that developers halved the
time needed to create new environments, from a full day to a few hours, after increasing
infrastructure automation with GKE. See the
[LiquidShare case study](https://cloud.google.com/customers/liquidshare) for the original account.

This is a vendor-published customer example about a particular migration, not a benchmark for local
emulation. Its value is the connection between environment work and developer capacity: reducing
provisioning time can return engineering time even when the environment remains provider-hosted.
Local Simile tests pursue the same operational outcome through a different mechanism. A CI job
starts an isolated compiled service, runs the test, and discards or preserves state according to the
test's purpose.

Record setup wait separately from test execution. Also record time lost to shared-state repair,
rate limits, provider outages, and flaky network dependencies. Those measurements identify whether
the constraint is infrastructure, test design, or service fidelity.

## Include security and incident response

Every real secret stored in test infrastructure creates lifecycle work. Teams must decide where it
may be stored, grant access, rotate it on schedule, revoke it when ownership changes, monitor its
use, and investigate possible exposure. These tasks consume security and engineering capacity even
when no incident occurs.

Primary-source incident reports make the response categories concrete:

- Codecov's
  [2021 postmortem](https://about.codecov.io/apr-2021-post-mortem/) describes a security task force,
  engineering and support work, third-party forensic analysts, key audits and rotation, Docker image
  changes, and build-pipeline changes after its Bash Uploader was modified.
- CircleCI's
  [January 2023 incident report](https://circleci.com/blog/jan-4-2023-incident-report/) describes
  host rotation, token revocation, external forensic support, customer tooling, and coordination
  with Atlassian and AWS after customer environment variables, keys, and tokens were exfiltrated.

Neither company publishes a complete monetary total in these reports. The evidence supports the
types of work that belong in a cost model; it does not support an invented dollar estimate. For
planning, record actual hours spent on secret inventory, routine rotation, incident exercises,
audit requests, and previous investigations. Keep expected incident cost as a separate, clearly
labeled assumption rather than mixing it into confirmed monthly spend.

Local emulation can reduce this category when routine tests use synthetic credentials and have no
standing trust into provider or production systems. It still needs dependency management, private
network binding, local state protection, and a policy for test artifacts. Read
[Risks of hosted test environments](/hosted-test-environment-risks) for the security boundary behind
those costs.

## Budget for fidelity and local resources

Simile requires ongoing engineering ownership. A service definition needs public contracts,
reviewed outcomes, declared state effects, and conformance evidence. Provider behavior evolves, so
teams must update definitions, investigate drift, and retain selected live tests that can detect
differences the local model cannot observe.

Budget for these recurring activities:

- authoring and reviewing public service definitions;
- compiling and distributing immutable service revisions;
- maintaining provider examples and conformance suites;
- running focused live checks against authoritative provider behavior;
- reviewing fidelity audit findings and proxy-learning evidence;
- updating the runtime as supported contract features evolve; and
- operating CI caches, artifacts, runners, and retained reproduction bundles.

Local compute is a real resource constraint. AWS's emulator repository tells source builders to
provide
[at least 100 GB of free disk for the Rust target directory](https://github.com/aws/aws-emulator#prerequisites)
and notes that a cold full build compiles one crate per AWS service. It recommends prebuilt packages
for most users and shared build caches for embedded use. Simile has a different scope, so this
number should not be applied to it. The example shows why local execution needs capacity planning.

Fidelity also has an opportunity cost. If a team repeatedly works around an inaccurate local model,
the emulator can consume more time than it saves. Track failures caused by definition gaps
separately from application failures. `simile audit` exposes schema-only and inferred behavior;
provider examples in [Conformance suites](/conformance-suites) create regression gates for reviewed
behavior. These mechanisms turn fidelity work into visible backlog and test results instead of
leaving it as an assumption.

## Make the decision with observed data

Compare a hosted baseline and a local pilot over the same period. Include the focused hosted suite
in the local model, because Simile adoption should not remove checks that only the provider can
supply.

| Measure | How to record it |
| --- | --- |
| Provider spend | Tagged compute, managed services, storage, networking, observability, and data transfer for test environments |
| CI and local resources | Runner minutes, machine class, cache and artifact storage, and incremental build time |
| Provisioning | Median and high-percentile elapsed time from request to usable isolated environment |
| Test feedback | Median and high-percentile duration from code change to actionable result |
| Access operations | Requests, approvals, rotations, revocations, and audit work |
| Environment reliability | Failed runs caused by outages, rate limits, shared state, or unavailable dependencies |
| Fidelity maintenance | Definition changes, conformance work, and investigations caused by emulator differences |
| Incident readiness | Time spent on inventory, rotation exercises, and actual response activity |

Do not turn every saved minute into payroll savings. Report time as capacity returned to feature
work unless headcount or contractor spend actually changes. Keep vendor case studies separate from
your own measured results. State which resources and teams the comparison includes, and document
any cost transferred to another budget.

Local emulation tends to provide the strongest return when tests run frequently, contributors need
independent state, provider setup is slow, external calls are metered, or CI must operate without
network access. Provider-hosted tests retain their value for real permissions, regional behavior,
network policy, provider capacity, and unresolved fidelity gaps. A broad local suite, a narrow
hosted suite, and a small production-safe smoke suite give teams a cost structure they can measure
without weakening the evidence required for release.

## Related resources

- [Design trade-offs](/design-trade-offs) - Understand the guarantees and maintenance obligations behind local execution
- [Risks of hosted test environments](/hosted-test-environment-risks) - Trace the trust paths that create security and response work
- [Fidelity audit](/fidelity-audit) - Find behavior that still needs provider review
- [Conformance suites](/conformance-suites) - Preserve reviewed provider behavior as a compatibility gate
- [CI testing](/ci-testing) - Supervise isolated local services and propagate test failures correctly
