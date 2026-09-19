# SpankBang

Downloads one SpankBang video and uploads the resulting MP4 through the shared `MegaRepository`.

```bash
mega-save spankbang \
  'https://spankbang.com/abc12/video/synthetic-example' \
  --remote mega:video/example
```

Use `--dry-run` to validate page access and media extraction without downloading or uploading:

```bash
mega-save spankbang \
  'https://spankbang.com/abc12/video/synthetic-example' \
  --remote mega:video/example \
  --dry-run
```

The command first tries the site HTML directly. If the site presents an anti-bot challenge, it falls back to Jina Reader for page extraction, validates the resulting media host against SpankBang's download CDN, and passes only that direct media URL to `yt-dlp`. The signed media URL is not printed.

Options include `--name`, `--keep-temp`, `--rclone`, `--yt-dlp`, `--format`, `--concurrent-fragments`, and `--workdir`. `--remote` is required even for dry runs so command invocations have one consistent shape.

Runtime dependencies: `curl`, `yt-dlp`, `ffmpeg`, and `rclone` (upload runs only).
