# mega-save wnacg

Save one **publicly accessible** [WNACG](https://www.wnacg.com/) `photos-slide` work as a single PDF, then upload it through the shared `MegaRepository` / `rclone` boundary.

```bash
mega-save wnacg 'https://www.wnacg.com/photos-slide-aid-248039.html' \
  -r mega:books/manga/r18/0

# Inspect the discovered image order without downloading, creating, or uploading a PDF.
mega-save wnacg --dry-run 'https://www.wnacg.com/photos-slide-aid-248039.html' \
  -r mega:books/manga/r18/0
```

By default, the command extracts the work title from the public page, removes the WNACG page/site boilerplate and leading creator credit, and uses it as the PDF basename. If that extraction is missing or ambiguous, it fails and asks for `--name`; it never falls back to an aid-based filename. `--name` itself must be a safe basename: empty names, `.`/`..`, absolute paths, and both `/` and `\\` separators are rejected. A name without `.pdf` receives that extension automatically (case-insensitive).

Other options: `--workdir /path/to/cache-root`, `--keep-temp`, `--rclone /path/to/rclone`.

Flow:

```
public work HTML → same-origin ordered `page_url` list → JPEG-backed PDF → MegaRepository → rclone
```

The command does **not** use browser cookies, credentials, DRM workarounds, login automation, or access-control bypasses. If a work or image responds with 401, 403, or 404, it fails with an explicit access-blocked error. Use it only for material you are authorized to copy.

Requires: `rclone` with the destination remote configured. Fully downloaded, decodable source pages are cached under a unique `$TMPDIR/mega-save-wnacg-<aid>-*/pages` directory by default; that temporary cache is removed when the command ends. With `--workdir /path/to/cache-root`, pages are cached under `/path/to/cache-root/mega-save-wnacg-<aid>/pages` and are retained after both failures and successful uploads so a later invocation can resume. The generated PDF is removed after a successful upload unless `--keep-temp` is set; `--keep-temp` affects only that PDF, not page-cache retention. Ensure the cache filesystem has enough space for the source pages and generated PDF.

Transient 5xx responses (including CDN 520), HTTP 429, connection failures, and request timeouts are retried up to four total attempts with 1s, 2s, and 4s backoff. Access-control responses (401, 403, 404) are never retried.
