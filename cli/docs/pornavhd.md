# mega-save pornavhd

[pornavhd.com](https://pornavhd.com/) single post → MEGA.

**Do not pass the post URL to yt-dlp directly.**

```
post HTML (curl) → recordplay embed → packer → hls2 → yt-dlp → MegaRepository
```

```bash
mega-save pornavhd 'https://pornavhd.com/YYYY/MM/DD/slug/' -r mega:video/r18/1/raikun
mega-save pornavhd --dry-run 'https://pornavhd.com/.../' -r mega:video/r18/0
```

Requires: `curl`, `yt-dlp`, `ffmpeg`, `rclone`.
