# mega-save-storage

MEGA 上の **ディレクトリ / ファイル操作**を `rclone` 経由でまとめたライブラリ。

サイト別ダウンローダ（`x` 等）はここに依存し、rclone を直接叩かない。

## 設計（関数型）

| 層 | 役割 | 純粋性 |
|----|------|--------|
| `path` | `RemotePath` の正規化・結合 | 純粋 |
| `op` | 操作の代数（データとしてのコマンド） | 純粋 |
| `rclone` | `Op` → 副作用（rclone 実行） | 非純粋（唯一の I/O 境界） |
| `repository` | ユースケース API（Op を組み立てて解釈） | 薄い合成 |

```text
RemotePath / Op  --pure-->  interpret(Rclone, Op)  --effect-->  Result
```

## API 概要

```rust
use mega_save_storage::{MegaRepository, RemotePath, Rclone};

let repo = MegaRepository::new(Rclone::default());
let dir = RemotePath::parse("mega:video/r18/0")?;

repo.ensure_reachable(&dir).await?;
repo.mkdir(&dir).await?;
repo.upload_file(Path::new("./a.mp4"), &dir).await?;
repo.move_path(&dir.join("a.mp4")?, &dir.join("b.mp4")?).await?;
repo.delete_file(&dir.join("b.mp4")?).await?;
// repo.purge_dir(&dir).await?; // 中身ごと
```

## 操作一覧

| 関数 | rclone |
|------|--------|
| `ensure_reachable` | `lsd <remote>:` |
| `mkdir` | `mkdir` |
| `upload_file` | `copy` local → dir |
| `delete_file` | `deletefile` |
| `delete_dir` | `rmdir`（空のみ） |
| `purge_dir` | `purge`（再帰） |
| `move_path` | `moveto` |
| `file_size` / `list_files` | `lsl` |
