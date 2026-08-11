# mega-save

Site-specific **video fetch → MEGA** tools.

```
mega-save/
  storage/     # mega-save-storage — MEGA ops repository (FP)
  x/           # mega-save-x — X/Twitter fetch CLI
  scripts/
```

## Architecture

```
site crate (x, …)  →  MegaRepository  →  Op/Program (pure)  →  rclone interpret (effect)
```

- **Acquisition is site-specific** (X: fxtwitter, not yt-dlp)
- **All MEGA mutations go through `storage`** — mkdir / upload / delete / move / purge
- Destination path is always explicit (`--remote`)

## storage (`mega-save-storage`)

| API | Meaning |
|-----|---------|
| `ensure_dir` | reachable + mkdir |
| `upload_file` / `upload_and_verify` | copy + size check |
| `delete_file` | deletefile |
| `delete_dir` | rmdir (empty) |
| `purge_dir` | purge (recursive) |
| `move_path` | moveto |
| `list_files` / `file_size` | lsl |

See [`storage/README.md`](storage/README.md).

## x (`mega-save-x`)

```bash
source scripts/env-build.sh
cargo build -p mega-save-x --release
./target/release/mega-save-x 'https://x.com/USER/status/ID' -r mega:video/r18/0
```

## Architecture lint (Semgrep)

責務境界を Semgrep で固定する（`semgrep/rules/architecture.yml`）。

| ルール（要約） | 守ること |
|----------------|----------|
| no rclone `Command::new` outside `storage/src/rclone.rs` | I/O 境界は interpret のみ |
| site (`x/**`) で `tokio::process` / `std::process::Command` 禁止 | MEGA は Repository 経由 |
| site から `interpret` / `run_program` 直接禁止 | Facade = `MegaRepository` |
| `path` / `op` / `error` は process 禁止 | 純粋層 |
| `repository.rs` は `Command::new` 禁止 | 合成のみ |

```bash
# install once: uv tool install semgrep
./scripts/semgrep.sh          # 本番ツリー（違反で exit != 0）
./scripts/semgrep-test.sh     # fixture が検出されることの自己テスト
```

CI: `.github/workflows/ci.yml` で `scripts/semgrep.sh` を実行。

## Build (this VPS)

```bash
source scripts/env-build.sh   # conda-forge gcc when system cc missing
./scripts/semgrep.sh
cargo test
cargo build --release
```
