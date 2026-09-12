# Release Packaging Design

## Goal

Package Clawcode release binaries for Windows, Linux, and macOS through a transparent CI matrix, with local packaging scripts and documented reproducibility checks.

## Scope

- Pin Rust toolchain `1.98.0` through `rust-toolchain.toml`.
- Build locked release binaries for the three target operating systems.
- Produce archive and SHA-256 checksum artifacts.
- Run locked tests on each CI operating-system runner.
- Provide local PowerShell and POSIX packaging scripts.
- Document same-environment reproducibility and cross-platform limits.

Out of scope: installers, code signing, notarization, publishing credentials, and automatic GitHub release creation.

## Architecture

GitHub Actions owns OS-specific build/test execution. Each job uses `cargo build --release --locked`, packages only the platform binary, and emits a checksum. Local scripts mirror the same build and checksum flow for supported host environments. `Cargo.lock` and `rust-toolchain.toml` make dependency and compiler inputs explicit.

## Validation

- YAML parses as valid workflow syntax.
- `cargo fmt --all -- --check` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo test --all-targets --all-features --locked -j 1` passes.
- Local packaging script builds, archives, checksums, and validates executable output.
- Repeating release build on the same environment produces matching binary checksum, subject to documented toolchain/platform conditions.

## Files

- `.github/workflows/release.yml`: OS matrix and artifact workflow.
- `rust-toolchain.toml`: explicit stable toolchain channel.
- `scripts/package.ps1`: Windows/local PowerShell packaging.
- `scripts/package.sh`: Linux/macOS packaging.
- `docs/reproducible-builds.md`: reproducibility procedure and limitations.
