# Simile

Simile is an Inttegro command-line application built around a small, explicit Rust workspace.

The initial commands expose the operational foundation:

```console
$ simile version
$ simile doctor
$ simile --output json doctor
```

## Architecture

Simile follows a command/operation boundary inspired by Cargo:

- `simile-cli` owns argument parsing, terminal rendering, logging and exit behaviour.
- `simile-core` owns application context, domain types and operations.
- Files under `commands/` are thin adapters. Behaviour belongs under `ops/`.
- Human output goes to stdout, diagnostics go to stderr, and JSON never contains ANSI colour.

Commands must not accumulate application logic. Add a command by declaring its arguments in
`simile-cli`, converting them into a core request, and delegating to a matching core operation.

## Development

The repository pins its minimum supported Rust toolchain. Common checks are available as Cargo
aliases:

```console
$ cargo test-all
$ cargo lint
$ cargo fmt --all --check
```

If `cargo-nextest` is installed locally, the checked-in profile can be used with:

```console
$ cargo nextest run --workspace --all-features
```

Release archives and installers are configured through `dist-workspace.toml`.
