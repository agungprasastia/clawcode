# Reproducible Builds

## Inputs

- Rust channel `1.98.0` is pinned in `rust-toolchain.toml`.
- Dependencies are pinned by `Cargo.lock`.
- Release builds use `cargo build --release --locked`.
- CI runs on fixed GitHub-hosted OS runners and stores SHA-256 checksums.

## Local check

Run the platform script twice in clean output directories:

- Windows: `pwsh -File scripts/package.ps1 -TargetDir target/release-package-1`
- POSIX: `sh scripts/package.sh target/release-package-1`

Repeat with `-2`, then compare `clawcode.sha256` files. Matching checksums demonstrate same-environment reproducibility.

## Limits

Binary bytes are expected to differ across operating systems, CPU targets, linker versions, and runner images. This project does not claim cross-platform byte identity. CI artifacts are independently checksummed per target. Code signing, notarization, and provenance attestations are outside MVP scope.
