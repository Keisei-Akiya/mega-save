# mega-save

Site-specific **video fetch → MEGA (`rclone`)** tools.

Monorepo layout (add a directory per site):

```
mega-save/
  Cargo.toml          # workspace
  scripts/env-build.sh
  x/                  # X / Twitter (mega-save-x)
  # pornavhd/         # future
  # youtube/          # future
```

## Principles

- **Acquisition is site-specific** — not “everything via yt-dlp”
- X: fxtwitter / vxtwitter → mp4 (**no yt-dlp**)
- MEGA: always `rclone` (remote usually `mega:`)
- Destination path is **required** (CLI `--remote`); no silent default upload path

## Build (this VPS)

System `gcc` is not installed; use conda-forge compilers:

```bash
source scripts/env-build.sh
cargo build --release
```

Binaries land in `target/release/` (e.g. `mega-save-x`).

## X crate

See [`x/README.md`](x/README.md).

```bash
./target/release/mega-save-x 'https://x.com/user/status/ID' -r mega:video/r18/0
```
