#!/usr/bin/env sh
set -eu

TARGET_DIR="${1:-target/release-package}"
cargo build --release --locked
mkdir -p "$TARGET_DIR"
cp target/release/clawcode "$TARGET_DIR/clawcode"
sha256sum "$TARGET_DIR/clawcode" > "$TARGET_DIR/clawcode.sha256"
"$TARGET_DIR/clawcode" --version >/dev/null
printf 'Packaged %s\n' "$TARGET_DIR/clawcode"
