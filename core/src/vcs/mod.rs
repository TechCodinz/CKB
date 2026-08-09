//! VCS integration module — Git history drift analysis

pub mod git_drift;
pub mod patch_transaction;
pub use git_drift::*;
pub use patch_transaction::*;
