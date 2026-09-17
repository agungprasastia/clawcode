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

## Providers

- **OpenAI & OpenAI-compatible**: endpoint `/chat/completions` (OpenAI, OpenRouter, DeepSeek, vLLM, dll.).
- **Anthropic**: native adapter endpoint `/v1/messages`.
- **Ollama**: native adapter endpoint `/api/chat`.

Autentikasi via environment variable (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`) atau secret references di konfigurasi.

## Commands

- `/plan` — beralih ke mode PLAN (read-only).
- `/build` — beralih ke mode BUILD (transactional execution).
- `/connect [provider]` — hubungkan provider (default atau provider spesifik).
- `/model <id>` — pilih model aktif (atau lihat model jika tanpa id).
- `/models` — daftar model tersedia.
- `/models refresh` — refresh model discovery dari provider.
- `/new <title>` — buat session baru.
- `/sessions` — daftar session tersimpan.
- `/agents` — dialog pemilih agent.
- `/themes` — dialog interaktif pemilih tema.
- `/theme <nama>` — ganti tema aktif.
- `/status` — status sesi & diagnostik sistem.
- `/keys` — cheatsheet pintasan keyboard.
- `/help` — daftar perintah.
- `/exit` — keluar aplikasi.

## Tools

Tool bawaan untuk agen AI:

- `read_file` — baca file (dukungan selector baris).
- `write_file` — buat atau overwrite file utuh (mode BUILD).
- `edit_file` — patch atau string replacement presisi (mode BUILD).
- `list_dir` — daftar isi direktori.
- `glob_search` — cari pola nama file.
- `grep_search` — cari teks dalam file workspace.
- `bash` — eksekusi command shell di workspace root (mode BUILD).
- `websearch` — cari web live via DuckDuckGo.
- `webfetch` — ambil konten URL web.
- `skill` — muat instruksi domain dari `skills/`.
- `question` — tanya klarifikasi interaktif ke user.
- `update_plan` — update status rencana multi-tahap (pending, in_progress, completed).

## Keybindings (TUI)

- `Tab` / `Shift+Tab` — toggle mode PLAN / BUILD.
- `Ctrl+C` — batalkan streaming / turn generasi aktif.
- `Ctrl+L` — bersihkan transcript layar.
- `Ctrl+X` — toggle cheatsheet shortcuts (`WhichKey`).
- `Esc` / `q` — tutup dialog aktif atau keluar saat input kosong.
- `PageUp` / `PageDown` — scroll transcript.
- `Shift+Up` / `Shift+Down` (atau `Ctrl`/`Alt` + panah) — scroll transcript per baris.
- `Up` / `Down` — navigasi riwayat input atau dialog.

Saat menu shortcut (`Ctrl+X`) aktif:
- `a` — buka dialog Agents.
- `t` — buka dialog Themes.
- `m` — buka dialog Models.
- `s` — buka dialog Status sistem.
- `p` / `b` — switch mode ke Plan / Build.
- `c` — clear transcript.

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
cargo bench --bench performance
cargo bench --bench first_frame
```

## Release packaging

- Windows: `pwsh -File scripts/package.ps1`
- Linux/macOS: `sh scripts/package.sh`

Scripts menghasilkan binary dan SHA-256 checksum. CI matrix Windows/Linux/macOS ada di `.github/workflows/release.yml`. Reproducibility procedure ada di `docs/reproducible-builds.md`.

## MVP boundaries

Core tidak bergantung pada provider tertentu. ACP, MCP, daemon, plugin system, remote client, desktop app, dan provider OAuth bespoke belum termasuk MVP.
