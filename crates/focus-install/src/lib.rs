//! The portable install (ADR 0004): where Focus keeps its files, which Focus processes are
//! running, and starting one from another. Shared by `focus-daemon` and `focus-ui`, which watch
//! over each other (ADR 0008).

mod instance;
mod launch;
mod paths;

pub use instance::{InstanceLock, is_held};
pub use launch::{install_folder, spawn_sibling};
pub use paths::Paths;
