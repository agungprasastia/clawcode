# Clawcode

Rust terminal AI coding assistant dengan streaming, session persistence, PLAN read-only, dan BUILD transactional.

## Requirements

- Rust `1.98.0` (pinned by `rust-toolchain.toml`)
- Windows, Linux, atau macOS

## Run

```text
cargo run -- --version
cargo run
```

Startup tidak membutuhkan network discovery. Provider discovery berjalan setelah aplikasi hidup dan memakai cache/backoff.

## Commands

- `/new <title>` — buat session.
- `/sessions` — daftar session.
- `/connect` — connect provider.
- `/models` — daftar model tersedia.
- `/models refresh` — refresh model discovery.
- `/exit` — keluar.

## Modes

PLAN bersifat read-only. Mutation filesystem dan shell mutatif ditolak.

BUILD mengikuti urutan `validate → snapshot → diff → policy → apply`. Write baru yang aman dapat berjalan tanpa approval; overwrite, delete, dan shell berisiko memerlukan approval. Snapshot mendukung undo/redo dan checksum validation.

## Configuration

Config utama memakai JSONC dengan schema version saat ini `1`. Contoh tersedia di `config/examples/`.

- Global: provider/model defaults.
- Project: override global untuk project aktif.
- Secret: gunakan `env:VARIABLE_NAME` atau `credential:ENTRY_ID`; plaintext API key ditolak.

Lihat `docs/diagnostics.md` dan `docs/migrations.md`.

## Development

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
```

## Release packaging

- Windows: `pwsh -File scripts/package.ps1`
- Linux/macOS: `sh scripts/package.sh`

Scripts menghasilkan binary dan SHA-256 checksum. CI matrix Windows/Linux/macOS ada di `.github/workflows/release.yml`. Reproducibility procedure ada di `docs/reproducible-builds.md`.

## MVP boundaries

Core tidak bergantung pada provider tertentu. ACP, MCP, daemon, plugin system, remote client, desktop app, dan provider OAuth bespoke belum termasuk MVP.
