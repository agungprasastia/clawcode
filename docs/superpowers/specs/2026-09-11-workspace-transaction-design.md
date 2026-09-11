# Workspace Transaction Design

## Tujuan

M6 menyediakan operasi workspace aman untuk PLAN dan BUILD. Semua file target dibatasi ke project root kanonis. BUILD memproses mutasi dalam urutan `validate → snapshot → diff → policy decision → approval → apply`. PLAN selalu read-only.

## Batas scope

M6 mencakup project root, read file terbatas, edit dan delete transaksional, snapshot checksum-validated, undo/redo, policy dan shell risk classification.

M6 tidak mencakup rollback shell command. Shell command hanya dieksekusi setelah policy dan approval terpenuhi.

## Modul

- `workspace/root.rs`: `WorkspaceRoot` mengkanonisasi project root dan memvalidasi target path. Target harus tetap di bawah root setelah normalisasi/canonicalization. Traversal dan symlink escape menghasilkan error workspace.
- `workspace/files.rs`: trait `FileSystem` kecil serta implementasi stdlib untuk read, write temporary, atomic rename, remove, canonicalize, dan metadata. Test fake filesystem dapat menolak operasi tertentu untuk menguji rollback.
- `workspace/snapshot.rs`: menyimpan state sebelum mutasi, ukuran bounded, dan checksum SHA-256. Restore menulis temporary file lalu rename atomik. Snapshot menyimpan before-state untuk undo dan after-state untuk redo.
- `workspace/policy.rs`: mendefinisikan mode `Plan`/`Build`, jenis operasi, risk classification, dan hasil `Allowed`/`ApprovalRequired`/`Denied`.
- `workspace/shell.rs`: mengklasifikasikan command dan memvalidasi working directory terhadap root. Command tidak dijalankan ketika policy membutuhkan approval atau menolak operasi.
- `workspace/mod.rs`: facade `Workspace` yang menjalankan pipeline mutasi dan mengekspos read, diff, apply, undo, redo, dan shell validation.

## Path boundary

`WorkspaceRoot::resolve` menerima path relatif project. Absolute path, traversal keluar root, dan target yang mengkanonisasi di luar root ditolak. Target baru divalidasi melalui parent kanonis agar path belum ada tetap aman. Read, edit, delete, restore, dan shell working directory melewati boundary ini.

## Read tools

Read menerima batas byte eksplisit. Jika file melampaui batas, hasil dipotong dan menandai truncation. Read tidak mengubah filesystem dan tersedia dalam PLAN maupun BUILD.

## Transactional mutation

Mutasi file menerima daftar edit/delete yang sudah tervalidasi. Pipeline tidak mengubah workspace sebelum snapshot, diff, policy, dan approval selesai.

1. Validate setiap path dan perubahan.
2. Snapshot semua before-state dan checksum.
3. Bangun diff dari snapshot menuju perubahan yang diminta.
4. Policy mengevaluasi mode, root boundary, overwrite/delete sensitivity, dan risk.
5. Bila hasil `ApprovalRequired`, caller harus memberi approval eksplisit; bila `Denied`, transaksi berhenti tanpa mutasi.
6. Apply setiap perubahan menggunakan write-temp lalu rename atomik. Kegagalan apply memulihkan item yang sudah berubah dari snapshot.

Transaction mengembalikan diff dan snapshot ID setelah apply berhasil. Undo merestore before-state; redo merestore after-state. Restore memvalidasi checksum sebelum menulis.

## Policy

PLAN menolak semua mutation dan shell execution. BUILD mengizinkan read dan mutasi aman di dalam root. Approval wajib untuk operasi luar root (tetap ditolak pada resolver), delete massal, overwrite file sensitif, dependency install, network mutation, privilege escalation, dan command destructive seperti reset/clean Git. Tidak ada policy `allow` yang menyembunyikan warning atau melewati approval wajib.

## Shell

Shell classifier mendeteksi bentuk command berisiko memakai token pertama dan argumen: delete massal, Git reset/clean, installer/package manager, network mutation, dan privilege escalation. Shell execution tidak memiliki rollback claim. Shell menerima working directory yang sudah lolos `WorkspaceRoot` dan hanya dapat berjalan di BUILD setelah approval jika dibutuhkan.

## Error handling

Semua boundary, policy, checksum, I/O, dan transaction failures menggunakan diagnostic kategori `Workspace`. Kegagalan validation, snapshot, diff, policy, atau approval tidak boleh meninggalkan mutasi. Kegagalan apply menjalankan restore best-effort untuk item yang telah diterapkan dan mengembalikan error rollback bila restore juga gagal.

## Testing

`tests/workspace_policy.rs` menguji canonical root, traversal, symlink escape, output read limit, PLAN mutation denial, policy approval, shell risk classification, transaction ordering, apply failure rollback, snapshot checksum rejection, atomic restore semantics, undo, dan redo. Tests memakai filesystem nyata pada temporary directory untuk boundary/symlink behavior dan fake `FileSystem` untuk kegagalan apply deterministik.

## Non-goals

Tidak ada shell rollback, execution sandbox, remote filesystem, file watcher, atau policy configuration language pada M6.
