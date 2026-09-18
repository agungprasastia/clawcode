# Clawcode

```text
  ____ _                         _      
 / ___| | __ ___      _____ ___   __| | ___ 
| |   | |/ _` \ \ /\ / / __/ _ \ / _` |/ _ \
| |___| | (_| |\ V  V / (_| (_) | (_| |  __/
 \____|_|\__,_| \_/\_/ \___\___/ \__,_|\___|
```

[![Rust](https://img.shields.io/badge/rust-1.98.0%2B-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue.svg?style=flat-square)](https://doc.rust-lang.org/edition-guide/)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen.svg?style=flat-square&logo=githubactions)](.github/workflows/ci.yml)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-informational.svg?style=flat-square)]()
[![Runtime](https://img.shields.io/badge/runtime-sync%20worker%20(zero--async)-success.svg?style=flat-square)]()
[![License](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue.svg?style=flat-square)](LICENSE)

**Clawcode** adalah asisten *AI coding* terminal berperforma tinggi berbasis bahasa pemrograman Rust. Didesain untuk pengembang dengan filosofi *keyboard-first*, *zero-async runtime overhead*, mitigasi mutasi berkas transaksional yang aman, pemantauan *side-by-side visual diff*, serta persistensi sesi menggunakan SQLite (WAL mode).

---

## Pratinjau Tampilan Terminal (TUI)

```text
┌───────────────────────────────────────────────────────────────────────────┐
│ clawcode v0.1.0 │ git:main │ provider:anthropic │ model:claude-3-7-sonnet │
├─────────────────────────────────────────────┬─────────────────────────────┤
│ TRANSCRIPT                                  │ TASK PLAN                   │
│                                             │                             │
│ User: Update autentikasi middleware         │ [x] Periksa src/auth.rs     │
│                                             │ [>] Validasi token expiry   │
│ Assistant: Menambahkan pengecekan token...  │ [ ] Verifikasi tes unit     │
│                                             │                             │
│ • Edit src/auth.rs (+2 -1)                  │                             │
│      12   fn verify_token(t: &str) -> bool {│                             │
│      13 -     t.len() > 10                  │                             │
│      13 +     let exp = parse_expiry(t)?;   │                             │
│      14 +     exp > Utc::now().timestamp()  │                             │
│      15   }                                 │                             │
├─────────────────────────────────────────────┴─────────────────────────────┤
│ [BUILD] > Ketik prompt atau /command (Ctrl+X: WhichKey)       Tokens: 4.2k│
└───────────────────────────────────────────────────────────────────────────┘
```

---

## Daftar Isi

- [Arsitektur Sistem](#arsitektur-sistem)
- [Fitur Unggulan](#fitur-unggulan)
- [Persyaratan Sistem](#persyaratan-sistem)
- [Instalasi & Panduan Cepat](#instalasi--panduan-cepat)
  - [Kompilasi dari Sumber](#kompilasi-dari-sumber)
  - [Konfigurasi Kunci Kredensial (API Key)](#konfigurasi-kunci-kredensial-api-key)
  - [Menjalankan Aplikasi](#menjalankan-aplikasi)
- [Konfigurasi Sistem (`clawcode.jsonc`)](#konfigurasi-sistem-clawcodejsonc)
- [Navigasi & Perintah TUI](#navigasi--perintah-tui)
  - [Perintah Teks (Slash Commands)](#perintah-teks-slash-commands)
  - [Pintasan Keyboard (Keybindings)](#pintasan-keyboard-keybindings)
  - [Menu Cepat WhichKey (`Ctrl+X`)](#menu-cepat-whichkey-ctrlx)
  - [Profil Agen Bawaan](#profil-agen-bawaan)
  - [Daftar Tema Tampilan](#daftar-tema-tampilan)
- [Spesifikasi 12 Tool Agen AI](#spesifikasi-12-tool-agen-ai)
- [Alur Transaksional Mode BUILD](#alur-transaksional-mode-build)
  - [Side-by-Side Visual Diff](#side-by-side-visual-diff)
- [Pengujian Mutu & Benchmark](#pengujian-mutu--benchmark)
- [Packaging & Distribusi Rilis](#packaging--distribusi-rilis)
- [Batasan Arsitektur & Non-Goals](#batasan-arsitektur--non-goals)
- [Lisensi](#lisensi)

---

## Arsitektur Sistem

Clawcode dioptimasi tanpa ketergantungan pada runtime async eksternal (zero `tokio` atau `async-std`). Seluruh koordinasi sistem berjalan di atas *worker thread* sinkron menggunakan saluran komunikasi baku Rust (`std::sync::mpsc`) dan *event bus* thread-safe.

```text
┌─────────────────────────────────────────────────────────────┐
│                    Terminal Frontend (TUI)                  │
│       Ratatui + Crossterm  •  16ms Event Polling (~60 FPS)   │
└──────────────────────────────┬──────────────────────────────┘
                               │ EventBus / std::sync::mpsc
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                   Synchronous Worker Runtime                │
│       RuntimeClient  •  State Machine  •  Task Coordinator   │
└──────────────┬───────────────────────┬──────────────────────┘
               │                       │
      ┌────────┴────────┐     ┌────────┴────────┐     ┌───────────────────────┐
      │  Workspace      │     │  Persistence    │     │  Provider Subsystem   │
      │  Sandbox        │     │  Layer          │     │  (Sync ureq + SSE)    │
      ├─────────────────┤     ├─────────────────┤     ├───────────────────────┤
      │ • Path Guard    │     │ • SQLite WAL    │     │ • OpenAI / OpenRouter │
      │ • SHA-256 Snap  │     │ • 3-Conn Pool   │     │ • Anthropic Native    │
      │ • Atomic Replace│     │ • Batch Writer  │     │ • Ollama Local        │
      │ • Policy Engine │     │ • Event Log     │     │ • Stream Normalizer   │
      └─────────────────┘     └─────────────────┘     └───────────────────────┘
```

Prinsip Inti Arsitektur:
- **Zero-Async Overhead**: Meniadakan beban runtime coroutine/async untuk latensi minimal dan jejak memori hemat.
- **Atomic File Transactions**: Operasi modifikasi berkas menggunakan berkas perantara temporer (*sibling tempfile*) dengan penggantian berkas atomik dan pemulihan instan saat terjadi interupsi.
- **Strict Bounded Resources**: Membatasi alokasi memori transcript (256 KiB) dan buffer stream (64 KiB), dengan pemotongan karakter strictly valid pada batas UTF-8 (*char boundary*).

---

## Fitur Unggulan

- **Inisialisasi Cepat & Offline-First**: Startup TUI instan (target ≤100 ms) tanpa memblokir koneksi jaringan. Cache metadata model lokal persisten dengan mekanisme *stale-while-revalidate* serta penanganan *backoff* eksponensial.
- **Dual Execution Modes**:
  - `PLAN`: Mode investigasi dan perencanaan *read-only*. Aman tanpa risiko perubahan kode atau eksekusi perintah shell berbahaya.
  - `BUILD`: Pipeline mutasi transaksional aktif (`validate` → `snapshot` → `diff` → `policy` → `apply`) dengan dukungan *checkpoint rollback* dan validasi hash SHA-256.
- **Visual Side-by-Side Diff**: Penyajian perubahan berkas berdampingan dua kolom (*split view*) di dalam transcript untuk operasi modifikasi kode (`edit_file`), lengkap dengan penomoran baris asli dan penanda warna kontras.
- **Provider Agnostic**:
  - Adapter native Anthropic Messages API (`/v1/messages`).
  - Provider OpenAI & penyedia kompatibel OpenAI (`/chat/completions`: OpenRouter, DeepSeek, vLLM, Groq, Mistral).
  - Eksekusi model lokal tanpa internet via Ollama (`/api/chat`).
- **Quiet Editorial TUI**: Antarmuka berbasis Ratatui dengan fokus pada kenyamanan membaca kode, penanganan perubahan ukuran layar (*resize-aware*), dan dialog WhichKey (`Ctrl+X`).
- **12 Tool Agen AI Bawaan**: Rangkaian perkakas otonom untuk inspeksi hierarki berkas, penyuntingan teks bedah (*surgical replacement*), pencarian web real-time, eksekusi shell terkendali, dan pelacakan target multi-tahap.
- **Keamanan Workspace Terproteksi**: Sandboxing berbasis penelusuran path kanonikal untuk mencegah *directory traversal attack* dan *symlink breakout*.
- **Persistensi Sesi SQLite**: Pool koneksi SQLite terkelola dalam mode WAL (*Write-Ahead Logging*) untuk menyimpan rekaman percakapan, metrik konsumsi token, dan snapshot pemulihan.

---

## Persyaratan Sistem

- **Rust**: Versi `1.98.0` atau lebih baru (terkunci via `rust-toolchain.toml`).
- **Sistem Operasi**: Windows 10/11, Linux (distro utama berbasis glibc atau musl), atau macOS (Apple Silicon / Intel).

---

## Instalasi & Panduan Cepat

### Kompilasi dari Sumber

```bash
# 1. Clone repositori
git clone https://github.com/owner/clawcode.git
cd clawcode

# 2. Bangun biner release teroptimasi
cargo build --release --locked
```

Biner terkompilasi akan berada di `target/release/clawcode` (atau `target\release\clawcode.exe` di lingkungan Windows).

### Konfigurasi Kunci Kredensial (API Key)

Konfigurasikan variabel lingkungan sesuai penyedia LLM yang digunakan:

```bash
# OpenAI atau endpoint kompatibel OpenAI
export OPENAI_API_KEY="sk-..."

# Anthropic Claude
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenRouter
export OPENROUTER_API_KEY="sk-or-..."

# Ollama Endpoint (opsional, default: http://localhost:11434)
export OLLAMA_HOST="http://localhost:11434"
```

### Menjalankan Aplikasi

```bash
# Menjalankan antarmuka interaktif TUI
cargo run

# Memeriksa versi rilis aplikasi
cargo run -- --version

# Menjalankan perintah non-interaktif langsung (CLI mode)
cargo run -- /models
cargo run -- /models refresh
cargo run -- /plan
cargo run -- /build
cargo run -- /connect anthropic
cargo run -- /new "Refaktorisasi Middleware"
```

---

## Konfigurasi Sistem (`clawcode.jsonc`)

Clawcode mendukung konfigurasi bertingkat dalam format JSONC (JSON dengan komentar dan *trailing commas*):

1. **Konfigurasi Global**: `~/.config/clawcode/clawcode.json` (preferensi bawaan pengguna di level sistem).
2. **Konfigurasi Proyek**: `.clawcode/clawcode.json` (diletakkan di root workspace, menimpa preferensi global).

### Contoh `clawcode.jsonc`

```jsonc
{
  "$schema": "https://clawcode.dev/schema/v1.json",
  "schema_version": 1,
  "model": "claude-3-7-sonnet-20250219",
  "endpoint": "https://api.anthropic.com/v1",

  // Penulisan kunci API plaintext ditolak validator konfigurasi.
  // Gunakan prefix 'env:NAMA_VAR' atau 'credential:ID_KEYRING'
  "api_key": "env:ANTHROPIC_API_KEY",

  // Skema warna antarmuka TUI aktif
  "theme": "clawcode-dark",

  // Konfigurasi endpoint penyedia LLM kustom
  "providers": {
    "openrouter": {
      "base_url": "https://openrouter.ai/api/v1",
      "api_key": "env:OPENROUTER_API_KEY",
      "models": {
        "anthropic/claude-3.5-sonnet": {
          "context_window": 200000,
          "max_output_tokens": 8192
        }
      }
    }
  }
}
```

> **Diagnostik Presisi**: Kesalahan sintaks pada berkas konfigurasi menyajikan koordinasi baris dan kolom 1-based secara akurat untuk mempermudah identifikasi masalah.

---

## Navigasi & Perintah TUI

### Perintah Teks (Slash Commands)

| Perintah | Deskripsi |
| :--- | :--- |
| `/plan` | Berpindah ke mode kerja **PLAN** (read-only, eksplorasi aman tanpa risiko mutasi). |
| `/build` | Berpindah ke mode kerja **BUILD** (eksekusi mutasi transaksional aktif). |
| `/connect [provider]` | Sambungkan ke provider default atau penyedia tertentu (`openai`, `anthropic`, `ollama`). |
| `/model <id>` | Ganti model aktif (atau buka dialog pemilih model bila tanpa parameter). |
| `/models` | Buka dialog modal penelusuran daftar model yang tersedia. |
| `/models refresh` | Paksa discovery ulang model dari server penyedia. |
| `/agents` | Buka dialog modal pemilih profil agen (`plan`, `build`, `review`, `compact`). |
| `/themes` | Buka dialog modal pemilih tema antarmuka visual. |
| `/theme <nama>` | Ganti skema tema tampilan langsung (contoh: `/theme catppuccin`). |
| `/new <judul>` | Buat sesi percakapan baru dengan judul yang ditentukan. |
| `/sessions` | Buka dialog modal penelusuran riwayat sesi percakapan. |
| `/clear` / `/home` | Bersihkan tampilan transcript aktif pada layar. |
| `/compact` | Ringkas transcript riwayat percakapan untuk menghemat pemakaian token konteks. |
| `/copy` | Salin seluruh transcript percakapan sesi aktif ke clipboard sistem. |
| `/status` | Tampilkan metrik runtime, diagnostik koneksi, dan rincian penggunaan token. |
| `/keys` | Buka cheatsheet pintasan keyboard (`WhichKey`). |
| `/help` | Tampilkan panduan ringkas seluruh perintah yang didukung. |
| `/exit` | Simpan sesi aktif ke database SQLite dan tutup aplikasi. |

### Pintasan Keyboard (Keybindings)

| Kombinasi Tombol | Tindakan |
| :--- | :--- |
| `Tab` / `Shift+Tab` | Beralih instan antara mode **PLAN** dan **BUILD**. |
| `Ctrl+C` | Hentikan proses streaming model aktif atau batalkan proses tool yang sedang berjalan. |
| `Ctrl+L` | Bersihkan tampilan transcript di layar. |
| `Ctrl+X` | Tampilkan menu pintasan cepat (**WhichKey**). |
| `Ctrl+V` | Tempel teks dari clipboard ke baris prompt. |
| `Esc` / `q` | Tutup dialog modal aktif / batalkan tindakan yang sedang berjalan. |
| `PageUp` / `PageDown` | Gulir transcript satu layar penuh ke atas / ke bawah. |
| `Shift+Up` / `Shift+Down` | Gulir transcript baris demi baris. |
| `Up` / `Down` | Navigasi riwayat prompt sebelumnya atau item pilihan dalam dialog modal. |
| `Enter` | Kirim instruksi prompt / konfirmasi pilihan item dialog. |

### Menu Cepat WhichKey (`Ctrl+X`)

Menekan tombol kombinasi `Ctrl+X` membuka menu tindakan cepat:

| Tombol Akses | Aksi / Dialog |
| :---: | :--- |
| `a` | Buka dialog modal **Agents** |
| `m` | Buka dialog modal **Models** |
| `t` | Buka dialog modal **Themes** |
| `s` | Buka dialog ringkasan **Status** runtime |
| `p` | Ubah mode kerja ke **PLAN** |
| `b` | Ubah mode kerja ke **BUILD** |
| `c` | Bersihkan transcript chat di layar |

### Profil Agen Bawaan

| Profil Agen | Mode Default | Deskripsi & Fokus Kerja |
| :--- | :---: | :--- |
| **Plan Agent** | `PLAN` | Penjelajahan repositori aman, pembedahan arsitektur, dan perancangan langkah kerja. |
| **Build Agent** | `BUILD` | Penyuntingan kode otonom, pemanggilan tool, dan mutasi terverifikasi. |
| **Review Agent** | `PLAN` | Audit kualitas kode sumber, verifikasi keamanan, dan evaluasi PR. |
| **Compact Agent** | `BUILD` | Eksekusi berfokus pada efisiensi token dengan respon ringkas dan latensi rendah. |

### Daftar Tema Tampilan

Clawcode menyediakan 8 skema palet warna bawaan:

| ID Tema | Nama Tema | Karakteristik Visual |
| :--- | :--- | :--- |
| `clawcode-dark` | **Clawcode Dark** *(Default)* | Aksen amber & teal kontras pada latar belakang arang pekat. |
| `crabcode-orange` | **Crabcode Orange** | Gradasi hangat jingga membara dan terumbu karang. |
| `catppuccin` | **Catppuccin Mocha** | Palet pastel lembut bernuansa mauve, biru laut, dan mocha. |
| `dracula` | **Dracula** | Kontras tajam ungu nokturnal, aksen merah muda, dan cyan. |
| `nord` | **Nord** | Palet dingin biru es Arktik, teal, dan malam kutub. |
| `gruvbox` | **Gruvbox Dark** | Nuansa retro hangat kayu gelap, krem, dan aksen oranye. |
| `tokyo-night` | **Tokyo Night** | Nuansa malam metropolitan bernada indigo dan biru neon. |
| `monokai` | **Monokai** | Palet legendaris abu-abu arang dengan aksen hijau, kuning, dan magenta. |

---

## Spesifikasi 12 Tool Agen AI

Sistem otonom Clawcode menggunakan 12 core tools dengan pembagian hak akses mode yang terisolasi:

| Nama Tool | Mode Akses | Cakupan & Fungsi |
| :--- | :---: | :--- |
| `read_file` | PLAN / BUILD | Membaca isi berkas dengan selektor baris presisi (`offset`, `limit`, format inline `path:10-50`). |
| `list_dir` | PLAN / BUILD | Mendaftar struktur berkas dan sub-direktori dalam workspace. |
| `glob_search` | PLAN / BUILD | Mencari berkas berdasarkan pola glob (contoh: `**/*.rs`, `src/**/*.json`). |
| `grep_search` | PLAN / BUILD | Pencarian pola teks cepat (case-insensitive) di seluruh berkas proyek. |
| `write_file` | BUILD | Membuat berkas baru atau menimpa seluruh isi berkas dari awal. |
| `edit_file` | BUILD | Modifikasi berkas bedah (*surgical replacement*) melalui pencocokan literal string eksak. |
| `bash` | BUILD | Mengeksekusi perintah shell pada direktori root workspace dalam batas sandbox. |
| `websearch` | PLAN / BUILD | Pencarian web real-time melalui DuckDuckGo Instant Answers. |
| `webfetch` | PLAN / BUILD | Mengambil konten URL web HTTP/HTTPS dan membersihkan tag markup menjadi teks rapi. |
| `skill` | PLAN / BUILD | Memuat modul panduan domain spesifik dari direktori `skills/`. |
| `question` | PLAN / BUILD | Mengajukan pertanyaan klarifikasi terstruktur kepada pengguna dengan opsi jawaban. |
| `update_plan` | PLAN / BUILD | Memperbarui checklist rencana kerja multi-langkah (*pending*, *in_progress*, *completed*). |

---

## Alur Transaksional Mode BUILD

Setiap operasi mutasi pada mode **BUILD** diproses melalui urutan verifikasi berkas atomik:

```text
Permintaan Mutasi ──► Validate ──► Snapshot ──► Diff Review ──► Policy Check ──► Apply / Revert
```

1. **Validate**: Memverifikasi jalur kanonikal target berkas guna mencegah celah *directory traversal* dan tautan simbolik (*symlink*) di luar direktori kerja.
2. **Snapshot**: Mengambil cadangan kondisi berkas saat ini ke dalam basis data SQLite bersama hash checksum SHA-256 untuk mendukung pemulihan (*undo/redo*).
3. **Diff Review**: Menghitung selisih modifikasi baris demi baris sebelum perubahan diterapkan.
4. **Policy Check**: Memeriksa batasan kebijakan keamanan. Operasi destruktif atau mutasi di luar direktori kerja mewajibkan konfirmasi eksplisit dari pengguna.
5. **Apply / Revert**: Menulis perubahan menggunakan berkas penampung sementara (*sibling temp file*) yang dipindahkan secara atomik. Jika terjadi kesalahan, pemulihan otomatis langsung dieksekusi.

### Side-by-Side Visual Diff

Saat tool `edit_file` dieksekusi, transcript menampilkan visualisasi diff berdampingan dua kolom:

```text
• Edit src/main.rs (+1 -1)
      10   fn main() {               │   10   fn main() {
      11 -     // old implementation │   11 +     println!("hello world");
      12   }                         │   12   }
```

---

## Pengujian Mutu & Benchmark

Seluruh repositori diverifikasi dengan standardisasi kualitas yang ketat:

```bash
# 1. Pengecekan pemformatan kode baku
cargo fmt --all -- --check

# 2. Linter statis (strict mode, zero warnings allowed)
cargo clippy --all-targets --all-features --locked -- -D warnings

# 3. Eksekusi seluruh rangkaian unit & integration tests
cargo test --all-targets --all-features --locked

# 4. Tolok ukur benchmark performa (render latency & first-frame throughput)
cargo bench --bench performance
cargo bench --bench first_frame
```

---

## Packaging & Distribusi Rilis

Skrip build otomatis menyusun artefak biner rilis dan menghasilkan berkas checksum SHA-256:

- **Windows (PowerShell)**:
  ```powershell
  pwsh -File scripts/package.ps1
  ```
- **Linux / macOS (POSIX Shell)**:
  ```bash
  sh scripts/package.sh
  ```

Dokumentasi lengkap mengenai prosedur pembangunan yang dapat direproduksi dijelaskan pada [`docs/reproducible-builds.md`](docs/reproducible-builds.md).

---

## Batasan Arsitektur & Non-Goals

Clawcode didesain secara spesifik sebagai biner terminal mandiri, portabel, dan instan. Hal-hal berikut sengaja berada di luar cakupan (*non-goals*):
- Daemon background process terpisah.
- Sistem plugin runtime pihak ketiga.
- Antarmuka web client atau pembungkus desktop GUI.
- Driver adapter khusus untuk endpoint yang sudah memenuhi spesifikasi protokol OpenAI.

---

## Lisensi

Didistribusikan di bawah ketentuan lisensi ganda [MIT](LICENSE) atau Apache-2.0. Rincian selengkapnya dapat ditemukan pada berkas `LICENSE`.
