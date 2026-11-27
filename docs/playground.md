---
title: Playground
description: Define and exercise a Simile service entirely in your browser.
---

The playground puts the service definition and its running emulator in one browser workspace. It
can compile OpenAPI or semantic TypeScript with HCL behavior, run operations, start stateful HTTP
sessions, inspect state, and export a native Simile workspace.

## Open the playground

```console
simile playground
```

Simile starts a loopback server on `http://127.0.0.1:4243`, opens the system browser, and prints the
URL. Press `Ctrl-C` in the terminal to stop it.

Use a stable loopback port when another local tool needs a predictable URL:

```console
simile playground --port 4243
```

Use `--no-open` to print the URL without opening a browser.

## What stays local

Compilation and emulation run through Simile's Rust engine in WebAssembly. Service definitions,
requests, responses, profiles, state, and checkpoints remain in the browser. The loopback server
only serves the playground application bundled with the Simile binary.

Browser storage belongs to the playground origin. Keeping the default port preserves the same
workspace across launches. A different port creates a different browser workspace. Export
important work before clearing browser data.

## Begin with a workspace

The default Orders workspace demonstrates authentication, stored resources, deferred work, and
webhooks. Select **Compile**, choose an operation, and run its request fixture.

Use **New workspace** to start from blank OpenAPI, semantic TypeScript, or the bundled Swagger
Petstore sample. The Petstore sample runs locally and never calls the public Petstore service.

## Move to native Simile

Export a runnable workspace when the definition is ready for a local listener or CI. The export
contains the Simfile, contract and HCL sources, compiled artifact, notes, and handoff commands.

The playground does not expose a TCP service endpoint. Use `simile register` and `simile start` for
an emulator that applications can call over HTTP.

<CardGroup cols={2}>
  <Card title="Define a service" icon="pen" href="/define-a-service">
    Structure the contract, behavior, and service configuration.
  </Card>
  <Card title="Run a service" icon="play" href="/service-lifecycle">
    Register and start the exported definition as a native emulator.
  </Card>
</CardGroup>
