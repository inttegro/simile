---
title: Playground
description: Author, compile, and exercise a Simile service in the browser.
---

The Simile playground combines service authoring and emulation in one browser workspace. Use it to
edit a contract and HCL behavior, compile the definition, run operations, inspect state, and export
the result for native Simile.

<Card title="Try an online playground" icon="arrow-up-right" href="https://similator.dela.dev">
  Open [Similator](https://similator.dela.dev) and start experimenting without installing Simile.
</Card>

## Prerequisites

- A recent [Simile installation](/downloads) for the bundled local playground
- A browser with WebAssembly, Web Workers, and IndexedDB enabled
- An OpenAPI or semantic TypeScript contract if you want to begin with your own service

## Launch the local playground

```console
simile playground
```

Simile serves the bundled application on `http://127.0.0.1:4243`, opens it in your browser, and
keeps the loopback server running until you press `Ctrl-C`.

Choose another loopback port when `4243` is already in use:

```console
simile playground --port 4343
```

To open the URL yourself, keep the server running without launching a browser:

```console
simile playground --no-open
```

The local server delivers the playground files. Compilation and emulation use Simile's Rust engine
compiled to WebAssembly and execute in browser workers.

## Understand browser persistence

The playground saves source files, notes, the latest compiled artifact, and reproduction cases in
IndexedDB. A runtime checkpoint is saved only when you select **Save checkpoint**. Restoring that
checkpoint is also explicit.

Browser storage is scoped to the complete origin: scheme, host, and port. Reopening
`http://127.0.0.1:4243` returns to the same local workspace. Starting the playground on port `4343`
creates a separate workspace. Changing domains has the same effect.

Export work you want to keep before changing the origin or clearing browser data.

## Build and exercise a service

Follow the same loop whether you start with the Orders example, a blank contract, or an imported
workspace:

1. **Author** — Edit the OpenAPI or semantic TypeScript contract. Add HCL when the service needs
   state changes, deterministic outcomes, deferred work, faults, or other observable behavior.
2. **Compile** — Select **Compile**. The playground validates the complete definition and keeps
   diagnostics beside the relevant source. **Describe** summarizes the service; **Audit** shows
   remaining fidelity gaps.
3. **Run** — Choose an operation and run its representative request or a saved fixture. A fixed seed
   makes repeated runs deterministic.
4. **Inspect** — Start an HTTP session to exercise stateful behavior. Inspect resources, the causal
   journal, deferred work, profiles, and checkpoints as the session changes.
5. **Export** — Download a runnable workspace ZIP when the definition is ready to leave the
   browser.

The browser session simulates the service locally. It does not open a TCP listener that another
application can call.

## Hand off to native Simile

The exported ZIP contains the service sources, compiled artifact, project notes, and a generated
`HANDOFF.sh` file. Extract it on a machine with the matching Simile version, review the files, then
run the handoff:

```console
unzip service.zip -d service
cd service
sh ./HANDOFF.sh
```

The handoff inspects and audits the compiled artifact, registers the service, performs any declared
principal setup, and starts the native listener. Native setup creates fresh profiles and
credentials; browser profiles and checkpoints remain in the browser workspace.

Your application can call the listener declared in the exported `Simfile`. Use the
[Console](/simile-web) to inspect the registered service and its live exchanges.

## Related resources

- [Define a service](/define-a-service) — Develop a contract and behavior definition from public
  service evidence
- [Lifecycle](/service-lifecycle) — Register, start, update, and remove native services
- [Console](/simile-web) — Inspect operations, exchanges, state, and deferred work
- [Run one operation](/run-operation) — Exercise a contract from the command line
