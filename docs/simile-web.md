---
title: Console
description: Inspect a local Simile service in the terminal or browser.
---

The console brings a service definition and its running state into one workspace. Use it to browse
operations, follow HTTP exchanges, inspect Stack state, review deferred work, and understand current
fidelity gaps.

## Open the console

Open the terminal interface for the first registered service:

```bash
simile console
```

Name a service to select it immediately:

```bash
simile console inttegro
```

Add `--web` to use the browser interface:

```bash
simile console inttegro --web
```

The service name is optional in both forms. When it is omitted, the console opens the first service
in the registry and keeps the service switcher available.

<Note>
  The console uses registered services. Run `simile register NAME --config URL_OR_FILE` before
  opening a service for the first time.
</Note>

## Service lifecycle

The console first looks for the selected service on its configured listener.

- If the matching compiled artifact is already running, the console attaches to it.
- If the listener is free, the console starts the service and restores its durable state.
- If another process owns the listener, the console stops with a conflict instead of attaching to
  an unrelated process.

Closing the console stops only a service that the console started. A service that was already
running remains running.

## Terminal interface

The terminal interface uses the same service model and runtime inspection endpoints as the browser.
It is useful over SSH and in environments where opening a browser would interrupt the workflow.

| Key | Action |
| --- | --- |
| `Tab`, `Shift+Tab`, `←`, `→` | Move between views |
| `j`, `k`, `↑`, `↓` | Scroll the current view |
| `s` | Open or close the service switcher |
| `Enter` | Select the highlighted service |
| `r` | Refresh runtime data |
| `q`, `Ctrl+C` | Close the console |

## Browser interface

The browser interface opens on a temporary loopback URL. It combines compiled service evidence with
live runtime activity.

<img
  className="block dark:hidden"
  src="/images/simile-web/overview-light.png"
  alt="Simile browser console showing a connected service and runtime telemetry"
/>
<img
  className="hidden dark:block"
  src="/images/simile-web/overview.png"
  alt="Simile browser console showing a connected service and runtime telemetry"
/>

The views have two evidence sources:

| Views | Source |
| --- | --- |
| Operations, scenarios, faults, profiles, and fidelity | The registered `.similec` artifact |
| Exchanges, State, Deferred work, and runtime charts | The running service's loopback inspector |

This distinction matters when diagnosing an emulation. The artifact explains what the service can
do; runtime views show what the service has done and retained locally.

### Operations

Open **Operations** to search the compiled HTTP surface. Each operation groups its route, declared
outcomes, representative payloads, and fidelity evidence.

<img
  className="block dark:hidden"
  src="/images/simile-web/operation-light.png"
  alt="An operation with its route, schemas, outcomes, and representative payloads"
/>
<img
  className="hidden dark:block"
  src="/images/simile-web/operation.png"
  alt="An operation with its route, schemas, outcomes, and representative payloads"
/>

Representative payloads explain the compiled contract. Calls made by your application appear under
**Exchanges**.

### Exchanges and state

**Exchanges** records calls handled by the local emulator and restores them when the service starts
again. **State** reads the running Stack and its payload-free causal journal without opening the
state database while the service owns it.

<img
  className="block dark:hidden"
  src="/images/simile-web/exchanges-light.png"
  alt="The Exchanges workspace showing requests handled by a local service"
/>
<img
  className="hidden dark:block"
  src="/images/simile-web/exchanges.png"
  alt="The Exchanges workspace showing requests handled by a local service"
/>

<img
  className="block dark:hidden"
  src="/images/simile-web/state-light.png"
  alt="The State workspace showing Stack resources and journal activity"
/>
<img
  className="hidden dark:block"
  src="/images/simile-web/state.png"
  alt="The State workspace showing Stack resources and journal activity"
/>

## Local security boundary

Runtime inspection is exposed only by services listening on a loopback address. Browser requests
must also originate from a loopback origin.

The inspector redacts sensitive JSON fields, headers, cookies, path parameters, and query
parameters. Opaque bodies are omitted, and bodies above 256 KiB are marked as truncated. Sanitized
exchanges remain in `$SIMILE_HOME/state/services/NAME.exchanges.redb` across restarts until you use
**Clear history**. The store has no automatic retention limit.

Response bodies and explicit State values can still contain application data. Keep the service and
console on a machine you trust.

## Related guides

- [State, identity, and replay](/state-and-replay) explains which continuity survives a restart.
- [State inspection](/state-inspection) explains live and offline ownership boundaries.
- [Reproduction bundles](/reproduction-bundles) preserve one exact execution outside the console.
- [Report an issue](/issue-reporting) verifies and submits a captured inconsistency.
- [Fidelity audit](/fidelity-audit) explains the findings shown in the console.
- [Describe a service](/describe-service) provides a non-interactive summary.
