# mega-save

Site-specific **video fetch → MEGA** tools.

```
mega-save/
  storage/     # mega-save-storage — MEGA ops repository (FP)
  x/           # mega-save-x — X/Twitter
  pornavhd/    # mega-save-pornavhd — pornavhd.com
  scripts/
  semgrep/
```

## Architecture

```
site crate (x | pornavhd | …)
  → fetch (site-specific)
  → MegaRepository → Op/Program (pure) → rclone interpret (effect)
```

- **Acquisition is site-specific** (X: fxtwitter; pornavhd: embed packer + yt-dlp)
- **All MEGA mutations go through `storage`**
- **Process spawn:** rclone only in `storage/src/rclone.rs`; yt-dlp only in `**/ytdlp.rs`; HTML curl in `**/curl_get.rs`
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

## Site CLIs

```bash
# X
./target/release/mega-save-x 'https://x.com/USER/status/ID' -r mega:video/r18/0

# pornavhd
./target/release/mega-save-pornavhd \
  'https://pornavhd.com/YYYY/MM/DD/slug/' \
  -r mega:video/r18/1/raikun
```

## Quality gates

| Script | What |
|--------|------|
| `./scripts/fmt.sh` | `cargo fmt --all`（適用） |
| `./scripts/clippy.sh` | `clippy -D warnings` |
| `./scripts/semgrep.sh` | 責務境界 |
| `./scripts/semgrep-test.sh` | fixture 自己テスト |
| `./scripts/check.sh` | **fmt check → clippy → semgrep → test** |

```bash
uv tool install semgrep   # once
source scripts/env-build.sh
./scripts/check.sh
```

CI (`.github/workflows/ci.yml`): **fmt / clippy / semgrep / test** を並列 job。

## Build (this VPS)

```bash
source scripts/env-build.sh
./scripts/check.sh
cargo build --release
```
