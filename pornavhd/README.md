# mega-save-pornavhd

[pornavhd.com](https://pornavhd.com/) 単発 post → MEGA。

**投稿 URL を yt-dlp に直接渡さない。**

```
post HTML → recordplay embed → packer decode → hls2 m3u8 → yt-dlp → MegaRepository
```

## Build

```bash
cd ~/repo/mega-save
source scripts/env-build.sh
cargo build -p mega-save-pornavhd --release
```

Requires: `yt-dlp`, `ffmpeg` on `PATH`, `rclone` + `mega` remote.

## Usage

```bash
./target/release/mega-save-pornavhd \
  'https://pornavhd.com/2026/07/25/raikun325_20/' \
  -r mega:video/r18/1/raikun

# resolve only
./target/release/mega-save-pornavhd 'https://pornavhd.com/.../' -r mega:video/r18/0 --dry-run
```

| Flag | Meaning |
|------|---------|
| `--remote` / `-r` | MEGA path (required) |
| `--name` | basename (default: URL slug `.mp4`) |
| `--dry-run` | print HLS URL only |
| `--yt-dlp` | yt-dlp binary (default `yt-dlp`) |
| `--format` | yt-dlp `-f` (default `bv*+ba/b`) |

## Modules

| file | role |
|------|------|
| `url` | post URL / slug (pure) |
| `page` | embed discovery (pure + fetch facade) |
| `packer` | Dean Edwards packer → `links.hls*` (pure) |
| `curl_get` | **only** process spawn for HTML GET (`curl`) |
| `ytdlp` | **only** process spawn for HLS→mp4 |
| `main` | wire + `MegaRepository` |
