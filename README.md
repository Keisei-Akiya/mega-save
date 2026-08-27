# mega-save

単一バイナリ **`mega-save`** で、サイト別取得 → MEGA（`rclone`）へ保存する。

```
mega-save/
  storage/     # mega-save-storage — MEGA repository (FP)
  cli/         # mega-save — subcommands: x | pornavhd | wnacg
  scripts/
  semgrep/
```

## Install / build

```bash
source scripts/env-build.sh   # this VPS (conda gcc if needed)
cargo build -p mega-save --release
# → target/release/mega-save
```

Requires on `PATH` depending on site: `rclone` (+ configured destination remote), and for pornavhd also `curl`, `yt-dlp`, `ffmpeg`.

## Usage

```bash
# X / Twitter (no yt-dlp)
mega-save x 'https://x.com/USER/status/ID' -r mega:video/r18/0

# pornavhd.com
mega-save pornavhd 'https://pornavhd.com/YYYY/MM/DD/slug/' -r mega:video/r18/1/raikun

# Public WNACG photo-slide work → one PDF (title-derived safe basename unless --name is supplied)
mega-save wnacg 'https://www.wnacg.com/photos-slide-aid-248039.html' -r mega:books/manga/r18/0

mega-save --help
mega-save x --help
mega-save pornavhd --dry-run 'https://pornavhd.com/.../' -r mega:video/r18/0
```

## Architecture

```
mega-save <site>
  → site module (cli/src/x | cli/src/pornavhd | cli/src/wnacg.rs)
  → MegaRepository → Op/Program (pure) → rclone interpret (effect)
```

| Layer | Role |
|-------|------|
| `cli/src/x` | fxtwitter/vxtwitter → mp4 |
| `cli/src/pornavhd` | embed packer → HLS → yt-dlp |
| `cli/src/wnacg.rs` | public photo-slide → ordered images → PDF |
| `storage` | mkdir / upload / delete / move / purge |

Process spawn boundaries:

- **rclone** only in `storage/src/rclone.rs`
- **yt-dlp** only in `**/ytdlp.rs`
- **curl** (HTML) only in `**/curl_get.rs`

## Quality gates

```bash
./scripts/check.sh   # fmt check → clippy → semgrep → test
```

| Script | What |
|--------|------|
| `fmt.sh` / `clippy.sh` | rustfmt / clippy `-D warnings` |
| `semgrep.sh` | architecture rules |
| `check.sh` | all of the above + `cargo test` |

CI: fmt · clippy · semgrep · test
