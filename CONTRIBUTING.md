# Contributing

Keep changes focused and preserve the boundary between command presentation and application
operations.

Before submitting a change, run:

```console
cargo fmt --all --check
cargo lint
cargo test-all
cargo doc --workspace --all-features --no-deps
```

Add unit tests for core behaviour and end-to-end tests for observable CLI behaviour. Command tests
should verify exit status, stdout and stderr independently.
