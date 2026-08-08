#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"

if ! pkl --version 2>/dev/null | grep '^Pkl 0\.31\.1 ' >/dev/null; then
  echo "error: Pkl 0.31.1 is required for non-skipping Velvet Glove validation" >&2
  exit 1
fi

just validate-plugins
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
cargo +1.85.0 check --locked --workspace --all-targets
cargo doc --locked --workspace --no-deps
cargo build --release -p velvet-glove --bin velvet-glove
scripts/regen-licenses.sh
git diff --exit-code -- THIRD_PARTY_LICENSES.md
