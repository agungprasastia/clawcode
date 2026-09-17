# Clawcode

```text
  ____ _                         _      
 / ___| | __ ___      _____ ___   __| | ___ 
| |   | |/ _` \ \ /\ / / __/ _ \ / _` |/ _ \
| |___| | (_| |\ V  V / (_| (_) | (_| |  __/
 \____|_|\__,_| \_/\_/ \___\___/ \__,_|\___|
```

[![Rust](https://img.shields.io/badge/rust-1.98.0%2B-orange.svg)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue.svg)](https://doc.rust-lang.org/edition-guide/)
[![CI](https://github.com/owner/clawcode/actions/workflows/ci.yml/badge.svg)]()
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)]()
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)]()

Terminal AI coding assistant berperforma tinggi berbasis Rust dengan streaming real-time, arsitektur dual-mode (PLAN & BUILD), side-by-side visual diff, session persistence berbasis SQLite, dan TUI keyboard-first.

---

## Daftar Isi

- [Fitur Utama](#fitur-utama)
- [Persyaratan Sistem](#persyaratan-sistem)
- [Instalasi & Memulai Cepat](#instalasi--memulai-cepat)
- [Konfigurasi](#konfigurasi)
- [Navigasi & Perintah TUI](#navigasi--perintah-tui)
  - [Perintah Teks (Slash Commands)](#perintah-teks-slash-commands)
  - [Pintasan Keyboard (Keybindings)](#pintasan-keyboard-keybindings)
  - [Menu Cepat WhichKey (`Ctrl+X`)](#menu-cepat-whichkey-ctrlx)
- [Tool Agen AI](#tool-agen-ai)
- [Alur Eksekusi Transaksional BUILD](#alur-eksekusi-transaksional-build)
  - [Side-by-Side Visual Diff](#side-by-side-visual-diff)
- [Pengujian & Benchmark](#pengujian--benchmark)
- [Packaging & Rilis](#packaging--rilis)
- [Batasan Arsitektur](#batasan-arsitektur)
- [Lisensi](#lisensi)

---

## Fitur Utama

- **Startup Cepat & Offline-First**: Target startup ≤100 ms tanpa memblokir network discovery. Cache model *stale-while-revalidate* dengan backoff eksponensial.
- **Dual Execution Modes**:
  - `PLAN`: Mode eksplorasi baca-saja (*read-only*) aman tanpa risiko mutasi berkas atau eksekusi shell destruktif.
  - `BUILD`: Pipeline mutasi transaksional (`validate` → `snapshot` → `diff` → `policy` → `apply`) dengan dukungan rollback (*undo/redo*) dan verifikasi checksum SHA-256.
- **Visual Side-by-Side Diff**: Tampilan diff berdampingan (*split column*) di dalam transcript TUI untuk mutasi berkas (`edit_file`), lengkap dengan penomoran baris asli, penanda `-` / `+`, dan blok warna kontras.
- **Provider Agnostic**:
  - OpenAI & OpenAI-compatible (`/chat/completions`: OpenRouter, DeepSeek, vLLM, Groq, dll.).
  - Anthropic native (`/v1/messages`).
  - Ollama native (`/api/chat`).
- **Quiet Editorial TUI**: Antarmuka berbasis Ratatui dengan fokus kejelasan diff, keyboard navigation, resize-aware, dan menu shortcut interaktif (`WhichKey`).
- **Autonomous Agent Tools**: Dilengkapi 12 core tools untuk eksplorasi workspace, manipulasi berkas presisi, eksekusi shell, pencarian web langsung, hingga pelacakan rencana multi-langkah.
- **Keamanan Workspace**: Sandboxing berbasis path kanonikal untuk mencegah path traversal dan symlink escape. Kebijakan izin eksplisit untuk operasi di luar proyek, penghapusan, atau shell sensitif.
- **Session Persistence**: Penyimpanan lokal berbasis SQLite untuk riwayat percakapan, metrik token, metadata model, dan snapshot patch berkas.

---

## Persyaratan Sistem

- **Rust**: `1.98.0` atau lebih baru (dikelola via `rust-toolchain.toml`).
- **Sistem Operasi**: Windows, Linux, atau macOS.

---

## Instalasi & Memulai Cepat

### Kompilasi dari Sumber

```bash
# Clone repository
git clone https://github.com/owner/clawcode.git
cd clawcode

# Bangun biner release
cargo build --release --locked
```

### Menjalankan Aplikasi

```bash
# Cek versi aplikasi
cargo run -- --version

# Jalankan TUI interaktif
cargo run

# Jalankan perintah langsung tanpa membuka TUI (CLI mode)
cargo run -- /models
cargo run -- /plan
cargo run -- /connect
```

### Konfigurasi API Key

Tentukan kunci API sesuai provider yang digunakan via environment variable:

```bash
# OpenAI / OpenAI-compatible
export OPENAI_API_KEY="sk-..."

# Anthropic
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenRouter
export OPENROUTER_API_KEY="sk-or-..."
```

---

## Konfigurasi

Clawcode mendukung konfigurasi JSON/JSONC bertingkat dengan schema versioning. Berkas konfigurasi divalidasi dan memetakan diagnostik baris/kolom bila terjadi kesalahan sintaks.

- **Global**: `~/.config/clawcode/clawcode.json` (preferensi bawaan seluruh sistem).
- **Project**: `.clawcode/clawcode.json` di root workspace (menimpa konfigurasi global).

### Contoh `clawcode.jsonc`

```jsonc
{
  "$schema": "https://clawcode.dev/schema/v1.json",
  "schema_version": 1,
  "model": "claude-3-7-sonnet-20250219",
  "endpoint": "https://api.anthropic.com/v1",
  "api_key": "env:ANTHROPIC_API_KEY",
  "theme": "clawcode-dark"
}
```

> **Catatan Keamanan**: Plaintext API key ditolak oleh validator konfigurasi. Gunakan prefix `env:NAMA_VAR` atau `credential:ENTRY_ID`.

---

## Navigasi & Perintah TUI

### Perintah Teks (Slash Commands)

| Perintah | Deskripsi |
| :--- | :--- |
| `/plan` | Beralih ke mode **PLAN** (read-only, eksplorasi aman). |
| `/build` | Beralih ke mode **BUILD** (eksekusi mutasi transaksional). |
| `/connect [provider]` | Hubungkan provider default atau provider spesifik (`openai`, `anthropic`, `ollama`). |
| `/model <id>` | Ganti model aktif (tampilkan model saat ini jika tanpa argumen). |
| `/models` | Buka dialog modal pemilih model yang tersedia. |
| `/models refresh` | Paksa pembaruan discovery model dari provider remote/lokal. |
| `/new <judul>` | Buat sesi percakapan baru dengan judul tertentu. |
| `/sessions` | Buka daftar dan riwayat sesi tersimpan. |
| `/agents` | Buka dialog modal pemilih agen AI spesialis. |
| `/themes` | Buka dialog modal pemilih tema tampilan antarmuka. |
| `/theme <nama>` | Ganti tema aktif secara langsung. |
| `/status` | Tampilkan status sesi aktif dan diagnostik runtime. |
| `/keys` | Buka cheatsheet pintasan keyboard (`WhichKey`). |
| `/help` | Tampilkan bantuan daftar perintah. |
| `/exit` | Simpan sesi aktif dan keluar dari aplikasi. |

### Pintasan Keyboard (Keybindings)

| Tombol | Aksi |
| :--- | :--- |
| `Tab` / `Shift+Tab` | Toggle instan antara mode PLAN dan BUILD. |
| `Ctrl+C` | Batalkan streaming model atau proses eksekusi tool seketika. |
| `Ctrl+L` | Bersihkan transcript tampilan layar. |
| `Ctrl+X` | Aktifkan pop-up pintasan cepat (`WhichKey`). |
| `Esc` / `q` | Tutup dialog modal aktif / batalkan input. |
| `PageUp` / `PageDown` | Gulir riwayat transcript layar penuh. |
| `Shift+Up` / `Shift+Down` | Gulir riwayat transcript per baris. |
| `Up` / `Down` | Navigasi riwayat input terminal atau baris item modal. |

### Menu Cepat WhichKey (`Ctrl+X`)

Saat menu pintasan aktif (`Ctrl+X`):

| Tombol | Target Dialog / Aksi |
| :---: | :--- |
| `a` | Buka dialog **Agents** |
| `m` | Buka dialog **Models** |
| `t` | Buka dialog **Themes** |
| `s` | Buka dialog **Status** runtime |
| `p` / `b` | Ubah mode kerja ke **PLAN** / **BUILD** |
| `c` | Bersihkan transcript chat |

---

## Tool Agen AI

Clawcode menyediakan 12 core tools bawaan yang dipanggil otonom oleh LLM:

| Tool | Mode Diizinkan | Deskripsi |
| :--- | :---: | :--- |
| `read_file` | PLAN / BUILD | Membaca berkas dengan selector baris (`offset`, `limit`). |
| `list_dir` | PLAN / BUILD | Memeriksa struktur berkas dan sub-direktori workspace. |
| `glob_search` | PLAN / BUILD | Mencari pola berkas menggunakan filter glob (misal: `**/*.rs`). |
| `grep_search` | PLAN / BUILD | Pencarian teks cepat di seluruh berkas proyek. |
| `write_file` | BUILD | Membuat berkas baru atau menulis ulang berkas secara utuh. |
| `edit_file` | BUILD | Modifikasi berkas presisi (surgical string replacement). |
| `bash` | BUILD | Mengeksekusi perintah shell pada root direktori workspace. |
| `websearch` | PLAN / BUILD | Pencarian web real-time melalui DuckDuckGo. |
| `webfetch` | PLAN / BUILD | Mengambil dan mengekstrak teks konten halaman web. |
| `skill` | PLAN / BUILD | Memuat modul panduan domain dari direktori `skills/`. |
| `question` | PLAN / BUILD | Mengajukan pertanyaan klarifikasi interaktif ke pengguna. |
| `update_plan` | PLAN / BUILD | Memperbarui visualisasi status checklist langkah kerja. |

---

## Alur Eksekusi Transaksional BUILD

Setiap mutasi pada mode **BUILD** melalui alur verifikasi ketat:

```text
Permintaan Mutasi ──► Validate ──► Snapshot ──► Diff Review ──► Policy Check ──► Apply / Revert
```

1. **Validate**: Memastikan target path berada dalam boundary kanonikal proyek (anti directory-traversal).
2. **Snapshot**: Mengambil cadangan kondisi berkas ke SQLite dengan hash SHA-256 untuk mendukung *undo/redo*.
3. **Diff Review**: Menghitung delta perubahan baris per baris.
4. **Policy Check**: Operasi berkas sensitif atau eksekusi perintah shell berisiko mewajibkan persetujuan eksplisit.
5. **Apply / Revert**: Menulis perubahan ke media simpan, atau mengembalikan berkas secara atomik jika terjadi kegagalan.

### Side-by-Side Visual Diff

Saat tool `edit_file` dieksekusi, transcript menampilkan visualisasi diff dua kolom (*side-by-side*):

```text
• Edit src/main.rs (+1 -1)
      10   fn main() {               │   10   fn main() {
      11 -     // old implementation │   11 +     println!("hello world");
      12   }                         │   12   }
```

---

## Pengujian & Benchmark

```bash
# Validasi format kode
cargo fmt --all -- --check

# Linter statis (strict)
cargo clippy --all-targets --all-features --locked -- -D warnings

# Jalankan seluruh rangkaian tes unit dan integrasi
cargo test --all-targets --all-features --locked

# Benchmark performa (render latency & frame throughput)
cargo bench --bench performance
cargo bench --bench first_frame
```

---

## Packaging & Rilis

Script build otomatis menyusun biner terkompilasi dan memproduksi hash SHA-256 untuk verifikasi rilis:

- **Windows**:
  ```powershell
  pwsh -File scripts/package.ps1
  ```
- **Linux / macOS**:
  ```bash
  sh scripts/package.sh
  ```

Prosedur build yang dapat direproduksi (*reproducible builds*) dijelaskan lengkap di [`docs/reproducible-builds.md`](docs/reproducible-builds.md).

---

## Batasan Arsitektur

Clawcode dirancang sebagai biner terminal mandiri, portabel, dan instan. Komponen berikut sengaja berada di luar cakupan MVP:
- Background daemon process terpisah.
- Sistem plugin eksternal runtime.
- Remote client, web interface, atau desktop GUI wrapper.
- Provider adapter kustom untuk endpoint yang sudah kompatibel dengan format OpenAI.
- Provider OAuth bespoke di dalam biner client utama.

---

## Lisensi

Didistribusikan di bawah ketentuan lisensi MIT atau Apache-2.0. Lihat berkas `LICENSE` untuk rincian lengkap.
