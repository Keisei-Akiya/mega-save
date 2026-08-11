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

## Build (this VPS)

```bash
source scripts/env-build.sh   # conda-forge gcc when system cc missing
cargo test
cargo build --release
```
