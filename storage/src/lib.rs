//! MEGA storage via rclone — FP ops + repository.
//!
//! ```text
//! path (pure) → op/Program (pure) → rclone::interpret (effect) → MegaRepository (API)
//! ```

mod error;
mod op;
mod path;
mod rclone;
mod repository;

pub use error::{StorageError, StorageResult};
pub use op::{ensure_and_mkdir, upload_into, Op, Outcome, Program, RemoteEntry};
pub use path::RemotePath;
pub use rclone::{interpret, parse_lsl, run_program, Rclone};
pub use repository::MegaRepository;
