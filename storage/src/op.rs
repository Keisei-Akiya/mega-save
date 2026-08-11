//! Algebra of storage operations (pure data).
//!
//! 副作用は持たない。`crate::rclone::interpret` が唯一の実行機。

use crate::path::RemotePath;
use std::path::PathBuf;

/// ストレージに対するコマンド（関数型の「式」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// remote ルートが応答するか（認証・到達確認）
    EnsureReachable { remote_root: RemotePath },
    /// ディレクトリ作成（既存可）
    MkDir { dir: RemotePath },
    /// ローカルファイルをディレクトリへ copy（ファイル名は local の basename）
    UploadFile { local: PathBuf, dest_dir: RemotePath },
    /// 単一ファイル削除
    DeleteFile { path: RemotePath },
    /// 空ディレクトリ削除
    DeleteDir { dir: RemotePath },
    /// ディレクトリを中身ごと削除
    PurgeDir { dir: RemotePath },
    /// ファイルまたはディレクトリの移動/リネーム
    MovePath { from: RemotePath, to: RemotePath },
    /// ディレクトリ直下の一覧
    ListFiles { dir: RemotePath },
    /// ディレクトリ内の名前でサイズ取得
    FileSize { dir: RemotePath, name: String },
}

/// 解釈結果（必要分だけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Unit,
    Size(Option<u64>),
    Listing(Vec<RemoteEntry>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub size: u64,
}

/// 小さなコンビネータ: 複数 Op を順序付きで保持（純粋）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Program(pub Vec<Op>);

impl Program {
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn pure(op: Op) -> Self {
        Self(vec![op])
    }

    pub fn and_then(mut self, op: Op) -> Self {
        self.0.push(op);
        self
    }

    pub fn append(mut self, other: Program) -> Self {
        self.0.extend(other.0);
        self
    }

    pub fn ops(&self) -> &[Op] {
        &self.0
    }

    pub fn into_ops(self) -> Vec<Op> {
        self.0
    }
}

/// よく使う合成（純粋ヘルパ）。
pub fn ensure_and_mkdir(dir: RemotePath) -> Program {
    let root = RemotePath::new(dir.remote(), "").expect("remote name already validated");
    Program::empty()
        .and_then(Op::EnsureReachable { remote_root: root })
        .and_then(Op::MkDir { dir })
}

pub fn upload_into(dir: RemotePath, local: PathBuf) -> Program {
    ensure_and_mkdir(dir.clone()).and_then(Op::UploadFile {
        local,
        dest_dir: dir,
    })
}
