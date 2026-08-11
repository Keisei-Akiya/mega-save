//! High-level storage repository — composes pure `Op`s and runs the interpreter.

use crate::error::{StorageError, StorageResult};
use crate::op::{self, Op, Outcome, Program, RemoteEntry};
use crate::path::RemotePath;
use crate::rclone::{self, Rclone};
use std::path::Path;

/// MEGA（rclone remote）へのリポジトリ。サイト crate はこれだけを使う。
#[derive(Debug, Clone)]
pub struct MegaRepository {
    rclone: Rclone,
}

impl MegaRepository {
    pub fn new(rclone: Rclone) -> Self {
        Self { rclone }
    }

    pub fn from_bin(bin: impl Into<String>) -> Self {
        Self::new(Rclone::new(bin))
    }

    pub fn rclone(&self) -> &Rclone {
        &self.rclone
    }

    /// 任意の Program を実行（低レベル出口）。
    pub async fn run(&self, program: Program) -> StorageResult<Vec<Outcome>> {
        rclone::run_program(&self.rclone, program).await
    }

    pub async fn run_op(&self, op: Op) -> StorageResult<Outcome> {
        rclone::interpret(&self.rclone, op).await
    }

    // ----- 統制されたユースケース（名前 = 意図） -----

    pub async fn ensure_reachable(&self, any_on_remote: &RemotePath) -> StorageResult<()> {
        let root = RemotePath::new(any_on_remote.remote(), "")
            .map_err(|e| StorageError::Other(e.to_string()))?;
        self.run_op(Op::EnsureReachable { remote_root: root })
            .await
            .map(|_| ())
    }

    pub async fn mkdir(&self, dir: &RemotePath) -> StorageResult<()> {
        self.run_op(Op::MkDir { dir: dir.clone() })
            .await
            .map(|_| ())
    }

    /// 到達確認 + mkdir を一連で。
    pub async fn ensure_dir(&self, dir: &RemotePath) -> StorageResult<()> {
        self.run(op::ensure_and_mkdir(dir.clone()))
            .await
            .map(|_| ())
    }

    pub async fn upload_file(&self, local: &Path, dest_dir: &RemotePath) -> StorageResult<()> {
        self.run_op(Op::UploadFile {
            local: local.to_path_buf(),
            dest_dir: dest_dir.clone(),
        })
        .await
        .map(|_| ())
    }

    /// ensure_dir のあと upload。
    pub async fn upload_file_ensured(
        &self,
        local: &Path,
        dest_dir: &RemotePath,
    ) -> StorageResult<()> {
        self.run(op::upload_into(dest_dir.clone(), local.to_path_buf()))
            .await
            .map(|_| ())
    }

    pub async fn delete_file(&self, path: &RemotePath) -> StorageResult<()> {
        self.run_op(Op::DeleteFile { path: path.clone() })
            .await
            .map(|_| ())
    }

    /// 空ディレクトリのみ。
    pub async fn delete_dir(&self, dir: &RemotePath) -> StorageResult<()> {
        self.run_op(Op::DeleteDir { dir: dir.clone() })
            .await
            .map(|_| ())
    }

    /// 中身ごと削除。
    pub async fn purge_dir(&self, dir: &RemotePath) -> StorageResult<()> {
        self.run_op(Op::PurgeDir { dir: dir.clone() })
            .await
            .map(|_| ())
    }

    pub async fn move_path(&self, from: &RemotePath, to: &RemotePath) -> StorageResult<()> {
        self.run_op(Op::MovePath {
            from: from.clone(),
            to: to.clone(),
        })
        .await
        .map(|_| ())
    }

    pub async fn list_files(&self, dir: &RemotePath) -> StorageResult<Vec<RemoteEntry>> {
        match self.run_op(Op::ListFiles { dir: dir.clone() }).await? {
            Outcome::Listing(v) => Ok(v),
            other => Err(StorageError::Other(format!(
                "expected Listing, got {other:?}"
            ))),
        }
    }

    pub async fn file_size(&self, dir: &RemotePath, name: &str) -> StorageResult<Option<u64>> {
        match self
            .run_op(Op::FileSize {
                dir: dir.clone(),
                name: name.to_string(),
            })
            .await?
        {
            Outcome::Size(s) => Ok(s),
            other => Err(StorageError::Other(format!("expected Size, got {other:?}"))),
        }
    }

    /// upload 後にサイズ照合。一致しなければエラー。
    pub async fn upload_and_verify(
        &self,
        local: &Path,
        dest_dir: &RemotePath,
        expected_bytes: u64,
    ) -> StorageResult<()> {
        let name = local
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| StorageError::Other("local path has no file name".into()))?
            .to_string();

        self.upload_file_ensured(local, dest_dir).await?;
        let remote = self.file_size(dest_dir, &name).await?;
        match remote {
            Some(n) if n == expected_bytes => Ok(()),
            Some(n) => Err(StorageError::Other(format!(
                "size mismatch after upload: local={expected_bytes} remote={n} name={name}"
            ))),
            None => Err(StorageError::Other(format!(
                "uploaded file not listed: {name} in {dest_dir}"
            ))),
        }
    }
}
