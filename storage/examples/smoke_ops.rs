use mega_save_storage::{MegaRepository, Rclone, RemotePath};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo = MegaRepository::new(Rclone::default().with_progress(false));
    let base = RemotePath::parse("mega:video/r18/0/_mega_save_storage_smoke")?;
    let local = Path::new("/tmp/mssmoke/hello.txt");
    let expected = std::fs::metadata(local)?.len();

    repo.ensure_dir(&base).await?;
    repo.upload_and_verify(local, &base, expected).await?;
    let a = base.join("hello.txt")?;
    let b = base.join("hello-moved.txt")?;
    repo.move_path(&a, &b).await?;
    assert_eq!(
        repo.file_size(&base, "hello-moved.txt").await?,
        Some(expected)
    );
    assert_eq!(repo.file_size(&base, "hello.txt").await?, None);
    repo.delete_file(&b).await?;
    repo.purge_dir(&base).await?;
    println!("smoke_ops ok bytes={expected}");
    Ok(())
}
