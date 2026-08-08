# Velvet Glove

Deferred linting and formatting for coding agents.

The hook executable lives in [`crates/velvet-glove`](crates/velvet-glove) and uses
explicit event subcommands. See the crate README for hook registration and
the boundary between Copier-owned protocol adapters and editable policy.

## Validate

```sh
cargo fmt --all -- --check
cargo +1.85.0 check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

Copier generated this workspace without running tasks or changing external
harness configuration.
