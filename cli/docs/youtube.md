# mega-save youtube

Public YouTube video → one MP3 → MEGA.

```bash
mega-save youtube 'https://www.youtube.com/watch?v=VIDEO_ID' -r mega:music
mega-save youtube 'https://youtu.be/VIDEO_ID' -r mega:music --name 'track.mp3'
mega-save youtube --dry-run 'https://www.youtube.com/watch?v=VIDEO_ID' -r mega:music
```

- URLs containing a playlist download only the specified video.
- The default filename is `YouTube title [video id].mp3`; use `--name` to override it.
- `--name` receives `.mp3` automatically when omitted.
- The command uses yt-dlp's `mweb` client and Node.js JS runtime to retrieve public YouTube media, then verifies the uploaded remote file size via `rclone`.

Requires: `yt-dlp`, `ffmpeg`, `node`, `rclone`.
