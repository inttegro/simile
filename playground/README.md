# Simile playground

Author a service, drop a `.similec`, and exercise it entirely in a browser. The
Vite/React app uses shadcn 4.23.0 (Base UI), CodeMirror, and the shared Rust Worker
API. The Orders starter follows `docs/define-a-service.md`; its Simfile references
the exact exported paths. A multi-file TypeScript starter and blank OpenAPI are
also available.

See [Playground architecture](ARCHITECTURE.md) for the screen structure, two-Worker
design, compilation and persistence flows, and a guide to the source files.

**New workspace → Swagger Petstore** loads a bundled conversion of
[the Petstore Swagger 2.0 sample](https://petstore.swagger.io/v2/swagger.json) to
OpenAPI 3.0.3. All 20 operations are retained. Try POST /store/order with the
place-order fixture, or GET /user/{username} in HTTP session. The sample includes
usage notes and its license; no requests are sent to the public Petstore. Source
provenance and conversion commands are in `src/templates/petstore/SOURCE.md`.

## Run locally

Use Node 22.12 or newer, pnpm 12.23.0, Rust 1.85.0, and the matching binding
tool. Run these commands from the repository root:

```sh
rustup toolchain install 1.85.0 --profile minimal --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.100 --locked
pnpm --dir playground install --frozen-lockfile
pnpm --dir playground dev
```

Open the local URL printed by Vite. `build:wasm` generates ignored bindings in
`playground/src/wasm/` and the standalone binding harness's `pkg/` directory.
Development and the default `build` use the debug WASM profile.

## Build and deploy

The playground is a static site. After installing the tools and dependencies
above, build the optimized WASM and app, then preview the result locally:

```sh
pnpm --dir playground build:production
pnpm --dir playground preview --host 127.0.0.1 --port 4173
```

Open `http://127.0.0.1:4173`. The production command uses Cargo's release profile
and writes the complete site to `playground/dist/`. Vite's preview server is for
local verification; publish the built files with your chosen static host.
See [Vite's static deployment guide](https://vite.dev/guide/static-deploy).

1. Upload the contents of `playground/dist/`, including its `assets/` directory,
   to the host's public directory. Use `index.html` as the directory index.
2. Serve the site over HTTPS with JavaScript module support. Serve `.wasm` files
   as `application/wasm` for
   [streaming compilation](https://developer.mozilla.org/en-US/docs/WebAssembly/Reference/JavaScript_interface/instantiateStreaming_static).
   Asset requests must return the actual files, rather than an HTML fallback.
3. Keep `index.html` fresh when publishing a new build and retain the assets it
   references. The app uses relative asset URLs and has no server-side routes.
4. Open the published URL, compile a starter, run a baseline, and start an HTTP
   session to confirm that the Worker and WASM assets load.

Compilation, execution, and persistence run in the browser. Hosting only needs to
serve static files; it needs no Simile backend, database, account-specific build
settings, or runtime secrets. Configure the host and domain outside this repository.
Browser storage belongs to the site's origin; export work before changing domains.

Run the browser suite against a published site by replacing the example URL:

```sh
pnpm --dir playground exec playwright install chromium
PLAYGROUND_BASE_URL=https://playground.example.com pnpm --dir playground test:e2e
```

Setting `PLAYGROUND_BASE_URL` skips the local preview server. Each test uses its
own browser context and local storage; simulated HTTP runs locally in WASM,
including the bundled Petstore sample.

## Workflow

- **Definition editor:** edit/create/rename/delete files while the runner stays
  alongside the source. Open **Composition** below the editor to select a Simfile
  or quick-mode contract, attach HCL modules, and record project notes. Directory
  uploads and ZIPs preserve relative paths. Choose an entry when imports are
  ambiguous. Renaming does not rewrite imports or Simfile paths.
- **Add operation / Guide:** open the contract, follow an OpenAPI or TypeScript
  example, and compile a new route without leaving the runner. Guide also opens
  baseline and HTTP tools and links to the relevant Simile documentation sections.
- **Format:** format YAML, JSON, TypeScript, and Markdown with their own parsers.
  HCL and Simfile formatting reindents lines while preserving comments, inline
  blocks, and heredoc contents. Formatting is undoable and preserves large JSON
  integer literals. Invalid documents show an error without replacing the text.
- **Describe and Audit:** inspect the public catalogue and artifact counters;
  filter fidelity findings and apply the shared strict acceptance rule.
- **Run:** synthesize or load a request, expand **Run settings** for a decimal seed and optional
  outcome/scenario, then capture a native reproduction bundle. Payload JSON remains
  Rust-produced text; binary bodies and files remain bytes. Each operation keeps
  its own request draft, so switching from POST to GET does not carry a body over.
  The selected fixture stays visible and is marked modified after request edits.
  Body-bearing fixtures are disabled when the compiled operation declares no body.
  A pasted body shows an inline explanation and a **Remove request body** action;
  removal preserves the other fields verbatim. In Orders, use create-order.json
  with POST /orders, and an empty request document (`{}`) with GET /orders.
- **HTTP session:** explicitly start/restart a compiled revision. Provision caller
  profiles, opt in to applying their credentials, send HTTP requests, inspect
  backend state and the journal, advance manual time, and step work. Repeated
  headers remain separate. Absent bodies differ from JSON `null`. Authentication,
  wire options, and session controls expand independently.
- **Reproduction:** save/import/export named cases, replay them, or run a suite.
  ZIPs contain native bundle files; they do not represent arbitrary session state.

Drag the divider, or focus it and use arrow keys, to resize the definition and
execution panes. Narrow screens stack them vertically. Problems, Composition,
Describe, Audit, Reproduction, and Guide open in a collapsible panel below the editor.
Action, compilation, formatting, and storage errors automatically open Problems.
Switching execution tabs preserves request inputs and results; new workspaces
and successful imports start a fresh baseline form. Results from an earlier
compiled revision are labelled.

Compilation uses a separate Worker, debounces edits, coalesces pending builds,
and discards stale responses. It cannot replace the running revision. Cancel
compilation to restart its Worker without losing the session. Runtime recovery is
explicit and discards unsaved runtime state while retaining source edits.
If the compiler Worker crashes or times out, its diagnostic remains visible and
the next edit, Compile, or artifact import uses a fresh Worker. The active service
and its runtime state stay intact; the failed compilation is not retried endlessly.

Artifact imports validate before switching modes. Artifacts do not recover source
files. Failed compilation/import/checkpoint validation keeps the active session.
The engine reports artifact-format compatibility; the UI does not hard-code it.

## Saving and checkpoints

IndexedDB autosaves source files, notes, the last compiled artifact, and saved
reproduction cases independently from runtime state. Only a completed transaction
marks a save successful. A failed save leaves edits and the live session usable.

**Save checkpoint** captures the running artifact, options, snapshot, and revision.
A delayed save cannot mark newer mutations saved. **Restore saved** validates in a
spare Worker before replacing the live session. Reload restores source work;
restoring runtime state requires an explicit action. File imports validate against
the running artifact. Restoring preserves Stack state, profiles, outbox, and lease
visibility deadlines, but creates fresh replay caches and fault progress and
invalidates old work handles. This is not exact runtime continuation.

Profile credentials are attached inside Rust, and issued values stay hidden until
revealed in the Profiles panel. State inspection excludes internal credential
records from its resource picker. Saved artifacts and checkpoints can contain
synthetic credentials: browser storage belongs to this origin, not a secret vault.

The workbench opens no TCP listener. Delays and disconnects display transport
plans; webhook success/failure simulates a recipient without network delivery.
Read-only outbox inspection does not lease work. Step leases causally ready items;
expired handles reject without side effects. Redelivery processes the next
available dead letter through the shared service logic.

## Native handoff

Export a runnable workspace ZIP. Quick mode adds and validates a Simfile first.
The ZIP includes notes, exact artifact bytes, and `HANDOFF.sh` with inspect/audit,
describe, register, required setup, and start commands using the service's name.
Registration compiles the complete configuration, including inline setup. Native
profiles are created separately; browser credentials/checkpoints are not copied
into the native registry. A real native listener create/read flow has been verified
using the exported Orders workspace.

## Verify

```sh
pnpm --dir playground typecheck
pnpm --dir playground lint
pnpm --dir playground test
pnpm --dir playground build
pnpm --dir playground exec playwright install chromium
pnpm --dir playground test:e2e
cargo build -p simile-cli --locked
python3 playground/scripts/check-handoff.py
```

Set `CHROMIUM_PATH` to use an installed Chromium instead. Browser tests exercise
the production bundle, real WASM, module Workers, IndexedDB reload, artifact
imports, explicit revision switching, and the Orders proof loop. Unit tests cover
stale compilations, delayed/failed saves, archive paths, exact values, and formatting.
Browser checks cover new OpenAPI/TypeScript operations, fixture selection, error
visibility, and cursor/text/control contrast in light and dark themes. Native
coverage: `cargo test -p simile-core --test playground --locked`. Keep running all
repository checks from `AGENTS.md` and both existing browser harnesses.
The handoff check consumes the actual browser downloads and needs port 4242 free.
It registers and provisions the exported workspace in a temporary native home,
verifies authenticated create/read over TCP, and replays the exported bundle.

Components are checked-in shadcn source. To add components, use
`pnpm --dir playground dlx shadcn@latest add ...`.
