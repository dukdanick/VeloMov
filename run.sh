#!/usr/bin/env bash
# Run VeloMov from the source tree (builds it first if needed). Arguments are passed through.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cargo build --release --quiet --manifest-path "$here/Cargo.toml"
exec "$here/target/release/velomov" "$@"
