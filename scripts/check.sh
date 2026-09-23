#!/bin/sh
# Same gates as CI. Run before every commit.
set -e
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -q --workspace
