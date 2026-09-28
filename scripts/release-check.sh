#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"

pkl_version=$(pkl --version 2>/dev/null | awk '/^Pkl /{print $2; exit}')
if [ -z "$pkl_version" ] ||
  [ "$(printf '%s\n' 0.31.1 "$pkl_version" | sort -V | head -n 1)" != 0.31.1 ]; then
  echo "error: Pkl 0.31.1 or newer is required for non-skipping Velvet Glove validation" >&2
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
