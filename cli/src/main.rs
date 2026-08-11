//! mega-save — single binary with site subcommands.

mod pornavhd;
mod x;

use clap::{Parser, Subcommand};
use tracing::Level;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "mega-save",
    about = "Download site videos and upload to MEGA via rclone. Site-specific fetch; shared storage repository.",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// X / Twitter (fxtwitter/vxtwitter → mp4). No yt-dlp.
    X(x::Args),
    /// pornavhd.com post → recordplay HLS → yt-dlp.
    Pornavhd(pornavhd::Args),
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(Level::INFO.into())
                .from_env_lossy(),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let result = match cli.command {
        Commands::X(args) => x::run(args).await,
        Commands::Pornavhd(args) => pornavhd::run(args).await,
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
