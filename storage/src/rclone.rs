//! rclone binary client + Op interpreter (only effectful module).

use crate::error::{StorageError, StorageResult};
use crate::op::{Op, Outcome, Program, RemoteEntry};
use crate::path::RemotePath;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, info};

/// rclone 実行設定（不変）。
#[derive(Debug, Clone)]
pub struct Rclone {
    pub bin: String,
    pub checkers: u32,
    pub transfers: u32,
    pub progress: bool,
}

impl Default for Rclone {
    fn default() -> Self {
        Self {
            bin: "rclone".into(),
            checkers: 4,
            transfers: 2,
            progress: true,
        }
    }
}

impl Rclone {
    pub fn new(bin: impl Into<String>) -> Self {
        Self {
            bin: bin.into(),
            ..Self::default()
        }
    }

    pub fn with_progress(mut self, on: bool) -> Self {
        self.progress = on;
        self
    }
}

/// 単一 Op を解釈する（非純粋）。
pub async fn interpret(rclone: &Rclone, op: Op) -> StorageResult<Outcome> {
    match op {
        Op::EnsureReachable { remote_root } => {
            run_ok(rclone, "lsd", &[remote_root.remote_root().as_str()]).await?;
            Ok(Outcome::Unit)
        }
        Op::MkDir { dir } => {
            // 既存でも成功扱いに近づける
            let _ = run_status(rclone, "mkdir", &[dir.as_rclone().as_str()]).await?;
            Ok(Outcome::Unit)
        }
        Op::UploadFile { local, dest_dir } => {
            upload_file(rclone, &local, &dest_dir).await?;
            Ok(Outcome::Unit)
        }
        Op::DeleteFile { path } => {
            run_ok(rclone, "deletefile", &[path.as_rclone().as_str()]).await?;
            Ok(Outcome::Unit)
        }
        Op::DeleteDir { dir } => {
            run_ok(rclone, "rmdir", &[dir.as_rclone().as_str()]).await?;
            Ok(Outcome::Unit)
        }
        Op::PurgeDir { dir } => {
            run_ok(rclone, "purge", &[dir.as_rclone().as_str()]).await?;
            Ok(Outcome::Unit)
        }
        Op::MovePath { from, to } => {
            run_ok(
                rclone,
                "moveto",
                &[from.as_rclone().as_str(), to.as_rclone().as_str()],
            )
            .await?;
            Ok(Outcome::Unit)
        }
        Op::ListFiles { dir } => {
            let text = run_stdout(rclone, "lsl", &[dir.as_rclone().as_str()]).await?;
            Ok(Outcome::Listing(parse_lsl(&text)))
        }
        Op::FileSize { dir, name } => {
            let text = run_stdout(rclone, "lsl", &[dir.as_rclone().as_str()]).await?;
            let size = parse_lsl(&text)
                .into_iter()
                .find(|e| e.name == name)
                .map(|e| e.size);
            Ok(Outcome::Size(size))
        }
    }
}

/// Program（Op 列）を左から順に実行。
pub async fn run_program(rclone: &Rclone, program: Program) -> StorageResult<Vec<Outcome>> {
    let mut out = Vec::with_capacity(program.ops().len());
    for op in program.into_ops() {
        out.push(interpret(rclone, op).await?);
    }
    Ok(out)
}

async fn upload_file(rclone: &Rclone, local: &Path, dest_dir: &RemotePath) -> StorageResult<()> {
    if !local.is_file() {
        return Err(StorageError::Other(format!(
            "local file missing: {}",
            local.display()
        )));
    }
    info!(from = %local.display(), to = %dest_dir, "rclone copy");
    let mut cmd = Command::new(&rclone.bin);
    cmd.arg("copy")
        .arg(local)
        .arg(dest_dir.as_rclone())
        .arg("--checkers")
        .arg(rclone.checkers.to_string())
        .arg("--transfers")
        .arg(rclone.transfers.to_string());
    if rclone.progress {
        cmd.arg("-P");
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let status = cmd.status().await.map_err(StorageError::from)?;
    if !status.success() {
        return Err(StorageError::Rclone {
            op: "copy",
            message: format!("exit {status}"),
        });
    }
    Ok(())
}

async fn run_ok(rclone: &Rclone, op: &'static str, args: &[&str]) -> StorageResult<()> {
    let (code, _stdout, stderr) = run_capture(rclone, op, args).await?;
    if code != 0 {
        return Err(StorageError::Rclone {
            op,
            message: stderr_or(code, &stderr),
        });
    }
    Ok(())
}

async fn run_status(rclone: &Rclone, op: &'static str, args: &[&str]) -> StorageResult<i32> {
    let (code, _stdout, stderr) = run_capture(rclone, op, args).await?;
    if code != 0 {
        debug!(op, %stderr, code, "rclone non-zero (soft)");
    }
    Ok(code)
}

async fn run_stdout(rclone: &Rclone, op: &'static str, args: &[&str]) -> StorageResult<String> {
    let (code, stdout, stderr) = run_capture(rclone, op, args).await?;
    if code != 0 {
        return Err(StorageError::Rclone {
            op,
            message: stderr_or(code, &stderr),
        });
    }
    Ok(stdout)
}

async fn run_capture(
    rclone: &Rclone,
    op: &'static str,
    args: &[&str],
) -> StorageResult<(i32, String, String)> {
    let mut cmd = Command::new(&rclone.bin);
    cmd.arg(op);
    for a in args {
        cmd.arg(a);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    debug!(bin = %rclone.bin, op, ?args, "rclone spawn");
    let out = cmd.output().await.map_err(StorageError::from)?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Ok((code, stdout, stderr))
}

fn stderr_or(code: i32, stderr: &str) -> String {
    let t = stderr.trim();
    if t.is_empty() {
        format!("exit {code}")
    } else {
        t.to_string()
    }
}

/// `rclone lsl` 1行パーサ（純粋）。
pub fn parse_lsl(text: &str) -> Vec<RemoteEntry> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            // SIZE YYYY-MM-DD HH:MM:SS.nnnnnnnnn name (name may contain spaces)
            let mut parts = line.split_whitespace();
            let size: u64 = parts.next()?.parse().ok()?;
            let _date = parts.next()?;
            let _time = parts.next()?;
            let name = parts.collect::<Vec<_>>().join(" ");
            if name.is_empty() {
                return None;
            }
            Some(RemoteEntry { name, size })
        })
        .collect()
}

/// テスト用: OsStr 列。
#[allow(dead_code)]
fn args_os(args: &[impl AsRef<OsStr>]) -> Vec<&OsStr> {
    args.iter().map(|a| a.as_ref()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lsl_line() {
        let t = "  1183987444 2026-08-11 17:17:22.000000000 mega-save-x-e2e-cli.mp4\n";
        let v = parse_lsl(t);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "mega-save-x-e2e-cli.mp4");
        assert_eq!(v[0].size, 1183987444);
    }
}
