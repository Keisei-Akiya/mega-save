# mega-save-x

Public X / Twitter video → local mp4 → MEGA (`rclone`).

**Does not use yt-dlp** for X. Flow matches Hermes skill `mega-video-save` / `references/x-twitter.md`:

1. Parse status id (and screen name if present)
2. `api.fxtwitter.com` → on failure `api.vxtwitter.com`
3. Pick highest-bitrate mp4
4. Download
5. `rclone copy` to `--remote`

## Build

```bash
cd ~/repo/mega-save
source scripts/env-build.sh   # conda-forge gcc on this VPS
cargo build -p mega-save-x --release
./target/release/mega-save-x --help
```

Requires: `rclone` on `PATH` with a working `mega` remote (or pass full remote path).

## Usage

```bash
mega-save-x 'https://x.com/user/status/123' --remote mega:video/r18/0
mega-save-x 'https://twitter.com/user/status/123' -r mega:video/r18/1/foo --dry-run
```

| Flag | Meaning |
|------|---------|
| `--remote` / `-r` | Destination, e.g. `mega:video/r18/0` (**required**) |
| `--name` | Output basename (default: `{user}_{id}.mp4`) |
| `--keep-temp` | Keep local file after upload |
| `--dry-run` | Resolve mp4 URL only; no download/upload |
| `--rclone` | rclone binary (default: `rclone`) |

Exit non-zero on failure. Prints a short summary on success.
