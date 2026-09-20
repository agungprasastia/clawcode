# Product Requirements Document (PRD) — Clawcode

**Versi Dokumen:** 1.0.0-PROD  
**Tanggal:** 20 September 2026  
**Status:** Diterima (Active Reference)  
**Klasifikasi:** Open-Source Public / Production Grade  

---

## 1. Ringkasan Eksekutif & Identitas Produk

### 1.1 Visi & Identitas Produk
**Clawcode** adalah AI Coding Assistant terminal-native modern berkinerja tinggi yang menggabungkan interaksi conversational cerdas dengan kontrol eksekusi kode deterministik. Clawcode hadir dalam bentuk **TUI (Terminal User Interface)** interaktif dan **CLI (Command Line Interface)** yang elegan, dirancang untuk menggantikan kebutuhan akan ekstensi IDE berat, runtime JavaScript/Electron yang boros sumber daya, serta tool cloud-tethered yang tidak transparan.

Clawcode dibangun dari nol dengan filosofi ketenangan editorial (*quiet editorial design*), keamanan berbasis transaksi (*transactional sandbox*), kecepatan mutlak (*instant startup*), dan portabilitas tanpa kompromi.

### 1.2 Tech Stack Utama
- **Bahasa Pemrograman:** Rust 1.98.0 (Pinned edition 2024 via toolchain pin).
- **TUI & Terminal Engine:** Ratatui `0.30` dan Crossterm `0.29` untuk manipulasi terminal raw-mode cross-platform.
- **Database & Persistence:** SQLite via `rusqlite 0.37` (bundled) yang berjalan dalam **WAL (Write-Ahead Logging) mode** untuk konkurensi lokal berkecepatan tinggi.
- **Networking & HTTP:** `ureq 2.10` dengan dukungan native TLS untuk HTTP streaming synchronous tanpa dependensi async library eksternal.
- **Concurrency & Concurrency Primitives:** Synchronous worker thread runtime murni menggunakan modul standar Rust (`std::sync::mpsc`, `std::sync::Mutex`, `std::sync::atomic`). **Zero async/tokio runtime pada core**.
- **Serialization & Hashing:** `serde`, `serde_json`, `sha2 0.10`.

### 1.3 Nilai Tambah Utama (Value Proposition)
1. **Instan & Sangat Ringan:** Membuka workbench dalam waktu $\le 100\text{ ms}$ (median lokal benchmark: $1.36\text{ ms}$) dengan konsumsi memori hanya $\sim 20\text{ MB}$, tanpa Node.js, Bun, Python, atau Electron runtime.
2. **Deterministic Safety Sandbox:** Menghilangkan ketakutan eksekusi liar AI melalui pemisahan tegas antara mode observasi murni (**PLAN**) dan mutasi transaksional bergaransi rollback (**BUILD**).
3. **Provider-Agnostic Terbuka:** Bebas vendor lock-in dengan dukungan langsung untuk penyedia native (Anthropic, Ollama) maupun generic OpenAI-compatible (DeepSeek, Groq, OpenRouter, self-hosted vLLM/LM Studio).
4. **Kualitas UI Sejajar OpenCode:** Menghadirkan kenyamanan visual editorial terminal premium: natural background tanpa zebra-bar kasar, baris tool inline ringkas, thought folding, dan floating dialog fuzzy.

---

## 2. Target Pengguna & Karakteristik

### 2.1 Profil Pengguna
1. **Terminal-Centric Developers:** Pengguna Neovim, Vim, Tmux, Zellij, atau CLI workflow yang menginginkan bantuan coding AI tanpa harus keluar dari terminal atau membuka editor GUI eksternal.
2. **System & Performance Engineers:** Pengembang yang menghargai efisiensi sumber daya mesin, menolak aplikasi berbasis web-view/Electron yang memakan gigabyte RAM, dan mengutamakan responsivitas instan.
3. **Security-Conscious & Enterprise Engineers:** Pengembang yang membutuhkan transparansi dan verifikasi mutlak atas setiap berkas yang diubah atau perintah shell yang dijalankan sebelum diaplikasikan ke sistem.
4. **Open-Source General Developers:** Pengembang lintas platform (Linux, macOS, Windows) yang membutuhkan asisten coding yang mudah dipasang, minim konfigurasi, dan deterministik.

### 2.2 Karakteristik Onboarding & Portabilitas
- **Clean Onboarding (Zero Config Startup):** Pengguna dapat langsung menjalankan binary `clawcode` di direktori proyek apa pun tanpa setup awal berbelit-belit. Jika belum ada konfigurasi, Clawcode menyajikan empty state editorial yang ramah dengan panduan shortcut langsung.
- **Zero Hardcoded Paths:** Semua path direktori sistem (config, cache, database, runtime) dideteksi secara dinamis sesuai standar spesifikasi direktori platform:
  - Linux/Unix: `$XDG_CONFIG_HOME/clawcode` atau `~/.config/clawcode`
  - macOS: `~/Library/Application Support/clawcode`
  - Windows: `%APPDATA%\clawcode`
- **Struktur Konfigurasi JSON/JSONC Familiar:**
  - Konfigurasi global (`clawcode.json` / `clawcode.jsonc`) dan project-level (`.clawcode/clawcode.json`).
  - Mendukung komentar JSONC dan penimpaan bertingkat (*project config overrides global config*).
  - Dilengkapi validasi skema ketat dengan diagnostik kesalahan yang eksplisit menunjukkan berkas, nomor baris, dan kolom (`file:line:column`).
  - Pengelolaan rahasia (API Keys) mengutamakan environment variables atau OS credential store, melarang penyimpanan token plaintext di dalam repositori.

---

## 3. Nilai Utama & Prinsip Desain (Preserved Foundations)

Lima pilar di bawah ini bersifat inviolable (mutlak dipertahankan dan tidak boleh dikompromikan):

### Pilar 1: Single Native Binary
- **Ukuran dan Dependensi:** Satu executable biner mandiri tanpa ketergantungan runtime dinamis seperti Node.js, Bun, Python, atau Electron.
- **Startup Super Cepat ($\le 100\text{ ms}$):** Startup path lokal bersifat murni offline tanpa network call blocking. Benchmark Criterion membuktikan first render dicapai dalam $1.36\text{ ms}$ median.
- **Jejak Memori Minimal ($\sim 20\text{ MB}$):** Mengalokasikan memori terikat (*bounded buffers*), menjaga konsumsi RAM tetap ringan bahkan saat streaming konteks percakapan panjang.

### Pilar 2: Deterministic Safety Sandbox
- **PLAN Mode (Read-Only Mutlak):**
  - Hanya mengizinkan inspeksi kode, pembacaan berkas (`read_file`, `list_dir`, `glob_search`, `grep_search`), perancangan arsitektur, dan perintah shell non-mutatif.
  - Setiap upaya modifikasi berkas (`write_file`, `edit_file`, `patch`) atau perintah shell berisiko mutasi otomatis ditolak oleh policy engine tanpa eksekusi.
- **BUILD Mode (Transactional Mutations):**
  - Mutasi berkas wajib mengikuti siklus deterministik:
    $$\text{validate} \longrightarrow \text{snapshot} \longrightarrow \text{diff} \longrightarrow \text{apply}$$
  - **Validate:** Verifikasi bahwa path berada dalam canonical project boundary (mencegah directory traversal dan symlink escape).
  - **Snapshot:** Pembuatan reverse patch atomik di SQLite sebelum berkas disentuh.
  - **Diff:** Penyajian diff interaktif terstruktur kepada pengguna.
  - **Apply & Rollback:** Penerapan perubahan ke disk. Jika terjadi error I/O atau kegagalan parsing, sistem secara otomatis mengeksekusi *automatic rollback* mengembalikan berkas ke kondisi semula secara instan.

### Pilar 3: SQLite Persistence Berbasis WAL Mode
- **Local WAL Mode (Write-Ahead Logging):** Mengaktifkan konkurensi pembacaan tinggi tanpa memblokir thread antarmuka TUI.
- **Multi-Session State Management:** Penyimpanan banyak sesi percakapan, metadata model, metrik inferensi, dan status workspace.
- **Append-Only Event Log:** Tabel `generation_events` bertindak sebagai *source of truth* tunggal untuk event streaming dan tool execution, memungkinkan rekonstruksi riwayat (*replay model*) secara presisi.
- **Undo/Redo Tanpa Pencemaran Git:** Snapshot rollback patch internal memungkinkan pembatalan modifikasi berkas tanpa harus memanipulasi riwayat commit git atau membuat internal git repository siluman.

### Pilar 4: Synchronous Worker Runtime
- **Arsitektur Thread Terisolasi:**
  - **UI Thread:** Menjalankan loop rendering Ratatui/Crossterm secara responsif pada target 60 FPS ($16.6\text{ ms}$ frame budget), menangani input keyboard pengguna dan event resize tanpa pernah terblokir oleh I/O.
  - **Worker Thread(s):** Thread khusus berbasis `std::sync::mpsc` yang menangani koneksi HTTP streaming LLM, pembacaan soket, dan eksekusi tool I/O secara sinkron.
  - **Writer Thread:** Background thread terpisah untuk penulisan batch ke SQLite WAL.
- **Bebas Async/Tokio:** Mencegah masalah kompleks seperti *async cancellation leaks*, *future dropping hazard*, dependensi pohon async yang bengkak, serta stall pada tokio scheduler thread-pool.

### Pilar 5: Multi-Provider & Custom Endpoints
- **Adapter Bawaan:** Dukungan kelas satu untuk Native Anthropic API, Ollama lokal, dan Generic OpenAI-Compatible API.
- **Dukungan Custom Endpoint Luas:** Kompatibel dengan DeepSeek, Groq, OpenRouter, Together AI, vLLM, LM Studio, serta proxy internal perusahaan.
- **Dynamic Discovery & Caching:** Deteksi model otomatis dengan strategi *stale-while-revalidate*, TTL cache lokal di SQLite, dan exponential backoff jika provider offline.
- **Capability Flags:** Pengenalan kapabilitas model yang terukur (dukungan streaming, reasoning/thinking, native tool-calling, kalkulasi token usage).

---

## 4. Standar UI/UX (OpenCode Parity)

Untuk menyamai dan melampaui standar kenyamanan visual tool modern seperti OpenCode, Clawcode menetapkan spesifikasi antarmuka terminal yang presisi:

```
+-------------------------------------------------------------------------------+
|  CLAWCODE  ·  [BUILD]    deepseek-coder · ws#1 (main)                         |
|  ● Ready  ·  Session active                                                   |
+-------------------------------------------------------------------------------+
|                                                                               |
|  User: Optimalkan fungsi cache di src/persistence/db.rs                       |
|                                                                               |
|  + Thought for 4s                                                             |
|                                                                               |
|  ● Read src/persistence/db.rs (12 lines)                                      |
|  ● Edit src/persistence/db.rs (+4 -2)                                         |
|    • src/persistence/db.rs                                                    |
|      -  let conn = Connection::open(path)?;                                   |
|      +  let conn = Connection::open_with_flags(path, OpenFlags::default())?;  |
|      +  conn.execute_batch("PRAGMA journal_mode = WAL;")?;                    |
|                                                                               |
|  Saya telah mengaktifkan mode WAL pada koneksi SQLite agar performa lebih     |
|  optimal dan mencegah contention pembacaan concurrent.                        |
|                                                                               |
+-------------------------------------------------------------------------------+
|  /plan                                                            :main       |
|  Switch to read-only Plan mode                                                |
+-------------------------------------------------------------------------------+
|  Enter Submit · Ctrl+C Cancel · / Commands · Ctrl+X Keys · Esc Back           |
+-------------------------------------------------------------------------------+
```

### 4.1 Chat Stream & Visual Hierarchy
- **Clean Natural Terminal Background:** Tidak menggunakan background zebra striping penuh (*full-width zebra bars*) yang melelahkan mata. Warna latar belakang mengikuti warna alami terminal pengguna (*default terminal background*) dengan panel borders yang halus.
- **Compact Inline Tool Rows:** Setiap eksekusi tool dirender sebagai baris tunggal yang ringkas dan informatif:
  - *Selesai / Sukses:* `● Read path/to/file (12 lines)` atau `● Edit src/main.rs (+5 -2)` menggunakan penanda hijau/teal.
  - *Sedang Berjalan:* `◐ Reading path/to/file` disertai animasi *compact wave spinner* yang halus.
  - *Gagal:* `× Edit src/main.rs · failed: file is read-only` menggunakan penanda merah/error.
  - *Interaktif / Expandable:* Tool baris yang menghasilkan output panjang (seperti perintah bash atau diff berkas besar) dapat di-*toggle* untuk dibuka atau dilipat menggunakan klik mouse atau shortcut keyboard.
- **Folding Thought / Reasoning Duration Blocks:**
  - Blok penalaran LLM (*chain-of-thought*) secara default dilipat (*collapsed*) ke dalam baris ringkas: `+ Thought for 4s` dengan warna amber/aksen lembut.
  - Pengguna dapat membuka blok penalaran menjadi `- Thought for 4s` untuk membaca keseluruhan argumen penalaran tanpa mengotori alur baca utama transkrip.
- **Clear Side-by-Side & Inline Diff Viewer:**
  - Penampil perbedaan berkas yang terintegrasi langsung di dalam TUI.
  - Menampilkan baris penambahan (`+` hijau) dan penghapusan (`-` merah) secara akurat dengan nomor baris referensi sebelum dieksekusi oleh pengguna.

### 4.2 Floating Modals (Centered Fuzzy Dialogs)
Semua dialog konfigurasi dan pemilihan dirender sebagai modal floating terpusat (*centered overlay*) dengan kotak pencarian fuzzy otomatis:
- **/models:** Dialog pemilihan model dan provider. Menampilkan daftar model yang tersedia, status koneksi aktif, label konteks window (misal `128k`, `200k`), dan penanda model saat ini.
- **/sessions:** Panel pengelola sesi percakapan. Dikelompokkan berdasarkan direktori workspace ID (`ws#<id>`), dilengkapi penanda sesi tersemat (*pinned* `📌`), indikator status generasi yang sedang aktif, dan waktu interaksi terakhir.
- **/themes:** Pemilih tema visual secara instan (Dark, Light, Amber, Calm, Catppuccin, Nord, Tokyo Night) dengan pratinjau warna langsung pada antarmuka.
- **/agents:** Modal penggantian peran dan mode kerja secara cepat (`plan` untuk perancangan read-only, `build` untuk eksekusi terverifikasi).
- **/keys:** Dialog bantuan ringkas (*which-key cheatsheet*) yang menampilkan daftar pintasan keyboard yang sedang aktif sesuai konteks.

### 4.3 Input Card, Autocomplete, & Status Bar
- **Multiline Input Card:** Input box adaptif dengan penanganan baris baru yang fleksibel, scrolling vertikal saat prompt panjang, penunjuk posisi kursor visual, dan status border aktif.
- **Slash Command Autocomplete:** Saat pengguna mengetikkan karakter `/`, Clawcode menampilkan daftar saran perintah secara instan (`/plan`, `/build`, `/model`, `/models`, `/models refresh`, `/sessions`, `/new`, `/clear`, `/themes`, `/keys`, `/status`, `/help`, `/exit`) lengkap dengan deskripsi fungsional.
- **Prompt History Recall:** Navigasi riwayat prompt sebelumnya menggunakan tombol panah `Up` dan `Down`. Mengingat draf teks pengguna yang belum dikirim saat navigasi riwayat dimulai.
- **Active Git Branch Display:** Menampilkan branch git aktif dari repositori kerja saat ini secara dinamis pada pojok kanan bawah input card (misal `:main`, `:feature/auth`), membantu pengembang tetap sadar akan konteks percabangan kode mereka.

---

## 5. Arsitektur Teknis

### 5.1 Diagram Arsitektur & Alur Data

```
+-------------------------------------------------------------------------------+
|                                  USER / TTY                                   |
+---------------------------------------+---------------------------------------+
                                        | (Raw Terminal Events via Crossterm)
                                        v
+-------------------------------------------------------------------------------+
|                            UI THREAD (Ratatui 60 FPS)                         |
|  - Layout Engine & Editorial Rendering                                        |
|  - Input Card, History Buffer, Slash Suggestions                              |
|  - Floating Dialogs (/models, /sessions, /themes, /agents, /keys)             |
|  - Non-blocking Event Polling (channel receiver)                              |
+-------------------+---------------------------------------^-------------------+
                    |                                       |
    (Submit Prompt) |                                       | (Stream & Tool
                    v                                       |  Update Events)
+-----------------------------------------------------------+-------------------+
|                        RUNTIME COORDINATOR (Sync mpsc)                        |
|  - SessionCoordinator (Concurrency control, Wake coalescing, Interrupts)     |
|  - EventBus (Fan-out hub, Replay from monotonic seq)                          |
+-------------------+-----------------------------------------------------------+
                    | (Spawns worker thread per active session)
                    v
+-------------------------------------------------------------------------------+
|                        WORKER THREAD (Sync Execution)                         |
|                                                                               |
|   +-----------------------+              +--------------------------------+   |
|   |   Provider Adapters   |              |        Workspace Engine        |   |
|   |  - Anthropic Native   |              |  - Canonical Path Boundary     |   |
|   |  - Ollama Native      |              |  - Plan Policy (Read-Only)     |   |
|   |  - OpenAI-Compatible  |              |  - Build Transactional Sandbox |   |
|   |  (Streaming via ureq) |              |  - Snapshot & Rollback Patches |   |
|   +-----------+-----------+              +---------------+----------------+   |
|               |                                          |                    |
|               \--------------------+---------------------/                    |
|                                    | (Append Events)                          |
+------------------------------------+------------------------------------------+
                                     |
                                     v
+-------------------------------------------------------------------------------+
|                       SQLITE PERSISTENCE WRITER THREAD                        |
|  - Dedicated background thread with mpsc receiver                             |
|  - SQLite Connection in WAL Mode (PRAGMA journal_mode = WAL)                  |
|  - Batched writes to: sessions, messages, generation_events, snapshots        |
+-------------------------------------------------------------------------------+
```

### 5.2 Rincian Modul Inti (Modular One-Crate Architecture)
1. **`crate::cli`:** Entry point parsing argumen baris perintah, pemilihan mode interaktif TUI atau eksekusi batch CLI non-interaktif.
2. **`crate::tui`:** Engine antarmuka pengguna berbasis Ratatui. Mengatur komposisi layar, penghitungan diff tampilan, manajemen tema warna, animasi spinner, dan rendering floating dialog.
3. **`crate::runtime`:**
   - `SessionCoordinator`: Mengontrol status sesi, mencegah balapan eksekusi (*race conditions*), menggabungkan permintaan generasi yang beruntun (*wake coalescing*), dan mengelola pembatalan (*interrupt/cancellation*).
   - `EventBus`: Hub distribusi event terurut berbasis nomor urut monotonik (`seq`).
   - `RuntimeClient`: Antarmuka sinkron aman yang digunakan oleh UI thread untuk berinteraksi dengan runtime tanpa menyentuh database secara langsung.
4. **`crate::provider` & `crate::adapters`:**
   - Kontrak trait `Provider` abstrak yang menormalisasi event streaming menjadi: `TextDelta`, `ReasoningDelta`, `ToolCallStart`, `ToolCallDelta`, `ToolCallEnd`, `ToolResult`, `Usage`, `Finish`, `Error`, dan `Cancelled`.
   - Implementasi adapter terpisah untuk `AnthropicAdapter`, `OllamaAdapter`, dan `OpenAiCompatibleAdapter`.
5. **`crate::workspace`:**
   - `RootDetector`: Menentukan batas canonical direktori proyek kerja.
   - `PolicyEngine`: Memeriksa izin operasi. Memastikan mode PLAN memblokir seluruh mutasi dan mode BUILD mewajibkan snapshot sebelum mutasi.
   - `SnapshotManager`: Menghitung unified diff, menyimpan status sebelumnya ke database, dan mengelola rollback atomik.
6. **`crate::persistence`:**
   - Pengelola koneksi database SQLite lokal.
   - Skema database versioned dengan migration engine otomatis (v1 $\rightarrow$ v2).
   - Menangani persistensi transkrip percakapan, cache model provider, status sesi, dan log event.
7. **`crate::config`:**
   - Parser JSONC kustom dengan pelaporan posisi error yang presisi.
   - Penggabungan konfigurasi bertingkat (*hierarchical merging*) antara konfigurasi global pengguna dan konfigurasi lokal proyek.
8. **`crate::platform`:**
   - Abstraksi spesifik sistem operasi (Windows, Linux, macOS) untuk integrasi clipboard sistem, deteksi branch git, eksekusi shell, dan pembacaan credential store secara native tanpa menyebarkan `#[cfg(target_os)]` ke seluruh modul core.

---

## 6. Roadmap Bertahap (Phased Roadmap)

Roadmap pengembangan Clawcode disusun secara bertahap dan berorientasi pada kualitas, di mana setiap fase harus memenuhi gerbang validasi (*acceptance gate*) sebelum melangkah ke fase berikutnya:

```
+-------------------------------------------------------------------------------+
|  Prioritas #1 (Immediate): Stabilisasi Bug & Reliability                      |
|  - Perbaikan desync pembatalan & sesi runtime                                 |
|  - Pemantapan koherensi multi-turn & persistensi urutan event                 |
+---------------------------------------+---------------------------------------+
                                        | (Gate 1 Passed)
                                        v
+-------------------------------------------------------------------------------+
|  Fase 1 (Next Milestone): Git Panel & Skills Library                          |
|  - Interactive Git Status & Diff Viewer terintegrasi                          |
|  - Sistem Skills modular berbasis direktori .clawcode/skills/                 |
+---------------------------------------+---------------------------------------+
                                        | (Gate 2 Passed)
                                        v
+-------------------------------------------------------------------------------+
|  Fase 2 (Mid-term Milestone): Browser OAuth Login                             |
|  - Local HTTP callback listener untuk integrasi autentikasi web               |
|  - Penyimpanan token aman di OS Credential Store tanpa copy-paste manual      |
+---------------------------------------+---------------------------------------+
                                        | (Gate 3 Passed)
                                        v
+-------------------------------------------------------------------------------+
|  Fase 3 (Advanced Milestone): Model Context Protocol (MCP)                    |
|  - Klien stdio JSON-RPC eksternal untuk integrasi tool ekosistem MCP          |
|  - Isolasi eksekusi tool eksternal dalam deterministic safety sandbox         |
+-------------------------------------------------------------------------------+
```

### Prioritas #1 (Immediate): Stabilisasi Bug & Reliability
- **Tujuan Utama:** Menghilangkan kelemahan sinkronisasi status, mencegah kondisi balapan saat pembatalan interaktif, dan menjamin persistensi riwayat percakapan multi-turn yang sempurna.
- **Rincian Pekerjaan:**
  1. **Runtime Session & Cancellation State Desync Fix:**
     - Menjamin bahwa saat pengguna menekan `Ctrl+C` atau mengirimkan sinyal interupsi, status sesi pada `SessionCoordinator`, `ClientSessionState`, dan SQLite langsung bertransisi serempak ke `Cancelled`/`Idle`.
     - Menghentikan proses pembacaan soket HTTP worker thread secara seketika dan membersihkan wake queue yang tertunda agar tidak memicu eksekusi generasi liar lanjutan.
  2. **Multi-turn Conversation Coherence & Sequence Persistence:**
     - Memastikan integritas nomor urut monotonik (`seq`) pada tabel `generation_events`.
     - Menjamin rekonstruksi transkrip percakapan multi-turn dari SQLite memuat seluruh riwayat panggilan tool, argumen, dan hasil eksekusi tanpa ada event yang terduplikasi atau tertinggal (*gap-free event replay*).
- **Quality Gate #1:**
  - Tes stres pembatalan cepat berulang kali (rapid cancel stress test) berjalan 100% tanpa hang atau zombie threads.
  - Skenario percakapan multi-turn 10+ putaran dapat dimuat ulang (*reload*) dari database dengan integritas pesan yang identik secara biner.

### Fase 1 (Next Milestone): Git Panel & Skills Library
- **Tujuan Utama:** Memperkaya kapabilitas alur kerja pengembang harian langsung di dalam terminal tanpa ketergantungan tool eksternal.
- **Rincian Pekerjaan:**
  1. **Interactive Git Panel / Status Dialog:**
     - Dialog status modal baru (`/git` atau integrasi di `/status`) yang menampilkan daftar berkas yang dimodifikasi, ditambahkan, atau belum dilacak (*untracked, staged, unstaged*).
     - Pratinjau diff langsung per berkas dengan navigasi keyboard.
     - Kemampuan melakukan staging/unstaging berkas interaktif (`Space`) dan memicu dialog commit pesan git langsung dari antarmuka Clawcode.
  2. **Skills Library (`.clawcode/skills/`):**
     - Pengenalan struktur direktori standar `.clawcode/skills/<skill-name>/SKILL.md` pada level repositori dan `~/.config/clawcode/skills/` pada level global.
     - Setiap skill berisi instruksi prosedural khusus, batasan peran, atau prompt operasional (misal: review kode keamanan, migrasi database, pembuatan unit test).
     - Penemuan dinamis (*dynamic auto-discovery*) dan pemanggilan eksplisit via slash command (`/skill <name>`) serta injeksi konteks cerdas oleh model saat relevan.
- **Quality Gate #2:**
  - Operasi git terisolasi dengan penanganan kesalahan yang aman, tidak pernah merusak working tree git pengguna.
  - Skill parser memvalidasi format markdown dan mengisolasi prompt injection tanpa melebihi batas token context window.

### Fase 2 (Mid-term Milestone): Browser OAuth Login
- **Tujuan Utama:** Menyederhanakan onboarding pengguna untuk provider cloud komersial (seperti GitHub Copilot atau Anthropic) tanpa kerumitan menyalin dan menempelkan API key secara manual.
- **Rincian Pekerjaan:**
  1. **Local Callback Listener:**
     - Membuka listener HTTP lokal temporer pada port acak lokal (`127.0.0.1:port`) yang menangani alur otorisasi OAuth berbasis PKCE.
     - Membuka browser default sistem secara otomatis ke halaman login provider yang dipilih.
  2. **Secure Token Storage:**
     - Menangkap authorization code dari redirect URL lokal, menukarkannya dengan access/refresh token, lalu menyimpan token secara terenkripsi ke dalam OS Credential Store (Windows Credential Manager, macOS Keychain, Linux Secret Service).
     - Menyediakan fallback manual yang anggun (*graceful prompt*) jika lingkungan terminal berjalan secara headless atau browser gagal dibuka.
- **Quality Gate #3:**
  - Alur autentikasi browser berhasil diverifikasi pada Windows, macOS, dan Linux desktop.
  - Tidak ada token atau data sensitif yang bocor ke berkas log atau transkrip terminal.

### Fase 3 (Advanced Milestone): Model Context Protocol (MCP)
- **Tujuan Utama:** Membuka integrasi dengan ekosistem tool eksternal yang terus berkembang melalui protokol resmi Model Context Protocol (MCP) dari Anthropic.
- **Rincian Pekerjaan:**
  1. **External stdio JSON-RPC Client:**
     - Membangun klien transport stdio berbasis standar JSON-RPC 2.0 untuk berkomunikasi dengan server MCP lokal (misal: server database PostgreSQL, browser automation, filesystem extensions).
     - Sinkronisasi registri tool MCP ke dalam antarmuka pemanggilan tool Clawcode.
  2. **Strict Security Sandboxing for MCP Tools:**
     - Semua panggilan tool yang berasal dari server MCP eksternal tetap tunduk pada policy engine Clawcode (memerlukan persetujuan eksplisit pengguna jika melakukan mutasi berisiko).
  3. **Prasyarat Implementasi:** Fase 3 hanya akan dimulai setelah Prioritas #1, Fase 1, dan Fase 2 telah terbukti 100% stabil dalam rilis publik.
- **Quality Gate #4:**
  - Eksekusi tool MCP via stdio berjalan stabil dalam synchronous worker architecture tanpa menyebabkan kebocoran memori atau pemblokiran rendering TUI.

---

## 7. Non-Goals Eksplisit & Kriteria Peninjauan Ulang

Untuk menjaga fokus rekayasa, Clawcode menetapkan batasan yang tegas mengenai apa yang **tidak akan dibuat** beserta kriteria rasional kapan batasan tersebut dapat ditinjau ulang:

### 7.1 Remote Client / Web UI / ACP Daemon
- **Status:** Non-Goal Eksplisit.
- **Alasan Teknis:**
  - Menjalankan daemon jaringan atau web server lokal secara signifikan memperbesar *attack surface* keamanan pada mesin pengembang (risiko eksploitasi DNS rebinding, cross-site request forgery terhadap endpoint lokal).
  - Membutuhkan bundling aset frontend web (HTML/JS/CSS bundler), yang bertentangan langsung dengan komitmen *single compact native binary*.
  - Menambah kompleksitas siklus hidup proses di latar belakang (*daemon process management*, deteksi status mati suri).
- **Kriteria Peninjauan Ulang:**
  - Hanya akan ditinjau ulang setelah rilis **v1.0 stabil** terbukti solid, dan HANYA jika ada kebutuhan mendesak untuk lingkungan *headless cloud container* atau *remote pair-programming* yang tidak dapat diakomodasi melalui SSH port-forwarding standar.

### 7.2 Migrasi Async Runtime (Tokio) pada Core
- **Status:** Non-Goal Eksplisit.
- **Alasan Teknis:**
  - Arsitektur synchronous worker threads saat ini (`std::sync::mpsc` + worker pools terdedikasi) telah terbukti sangat deterministik, bebas dari kompleksitas async race conditions, hemat memori ($\sim 20\text{ MB}$), dan menjamin render loop TUI 60 FPS bebas jank.
  - Memasukkan Tokio ke dalam core akan membengkakkan ukuran biner, memperpanjang waktu kompilasi, serta membuka risiko *async cancellation hazard* saat pengguna membatalkan streaming secara tiba-tiba.
- **Kriteria Peninjauan Ulang:**
  - Hanya akan ditinjau ulang jika pada Fase 3 (MCP Integration) ditemukan bukti terukur bahwa integrasi transport MCP lanjutan (seperti Server-Sent Events / SSE concurrent sockets) mutlak tidak dapat ditangani secara efisien oleh worker threads sinkron.

### 7.3 GUI / Desktop Application (Electron / Tauri)
- **Status:** Non-Goal Permanen.
- **Alasan Teknis:**
  - Identitas fundamental Clawcode adalah **terminal-native assistant**. Membangun aplikasi jendela desktop terpisah mengaburkan fokus produk dan bersaing secara tidak perlu dengan editor GUI umum.
- **Kriteria Peninjauan Ulang:**
  - Tidak ada. Clawcode akan selamanya menjadi alat bantu berbasis terminal (TUI & CLI).

---

## 8. Metrik Keberhasilan & Quality Gates

### 8.1 Target Metrik Kuantitatif
| Kategori | Parameter Pengukuran | Ambang Batas (Target) | Status Baseline Saat Ini |
| :--- | :--- | :--- | :--- |
| **Kecepatan** | Warm/Local TUI First Render | $\le 100\text{ ms}$ | **$1.36\text{ ms}$ median** (Pass) |
| **Kecepatan** | Stream Delta Coalescing (1,024 deltas) | $\le 1.0\text{ ms}$ | **$242.0\text{ µs}$ median** (Pass) |
| **Kecepatan** | SQLite Batch Write (64 pesan) | $\le 10.0\text{ ms}$ | **$2.62\text{ ms}$ median** (Pass) |
| **Kecepatan** | Redraw Full TUI (120x40 viewport) | $\le 16.6\text{ ms}$ (60 FPS) | **$4.23\text{ ms}$ median** (Pass) |
| **Memori** | RAM Footprint saat Idle / Active Base | $\le 25\text{ MB}$ | **$\sim 20\text{ MB}$** (Pass) |
| **Biner** | Ukuran Single Standalone Binary | $\le 30\text{ MB}$ stripped | **$\sim 18\text{ MB}$** (Pass) |
| **Safety** | Kebocoran Path Boundary (Traversal) | **0 insiden** (0%) | **0 insiden** (Pass) |
| **Safety** | Kegagalan Rollback saat Edit Error | **0 insiden** (0%) | **0 insiden** (Pass) |

### 8.2 Production Quality Gates & Release Checklist
Sebelum setiap versi rilis publik dipublikasikan, seluruh tahapan gerbang kualitas berikut wajib berstatus hijau (*Passed*):

1. **Strict Linting & Zero Warnings:**
   - `cargo fmt --all -- --check` wajib rapi tanpa deviasi formatting.
   - `cargo clippy --all-targets --all-features --locked -- -D warnings` wajib bersih dengan toleransi 0 warning.
2. **Comprehensive Test Suite Matrix:**
   - 100% lulus seluruh unit tests, integration tests, dan property tests pada tiga platform target utama di GitHub Actions CI:
     - `ubuntu-latest` (Linux x86_64)
     - `macos-latest` (macOS Apple Silicon & Intel)
     - `windows-latest` (Windows x86_64 MSVC)
3. **Deterministic Sandbox Validation:**
   - Uji otomatis membuktikan bahwa pada mode **PLAN**, tidak ada berkas filesystem yang berubah, bahkan jika model mencoba mengeluarkan perintah mutasi berbahaya.
   - Uji otomatis membuktikan bahwa pada mode **BUILD**, transaksi pembuatan snapshot dan perhitungan diff selalu mendahului penerapan perubahan (*apply*).
4. **Reproducible Builds & Binary Integrity:**
   - Biner rilis dihasilkan melalui pipeline build terisolasi dengan validasi checksum SHA-256 yang konsisten dan terverifikasi.
5. **No Regressions on Diagnostics:**
   - Semua kegagalan konfigurasi (JSONC malformed, unknown fields) wajib menghasilkan pesan error yang menyebutkan berkas dan posisi koordinat baris/kolom secara presisi tanpa memicu panic proses.
