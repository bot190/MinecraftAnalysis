//! Loss-preserving Minecraft Forge world migration primitives.

pub mod chunk_blocks;
pub mod convert;
pub mod coverage;
pub mod document_conversion;
pub mod explanation;
pub mod inventory;
pub mod manifest_authoring;
pub mod nbt;
pub mod pipeline;
pub mod preflight;
pub mod profile;
pub mod progress;
pub mod region;
pub mod region_conversion;
pub mod registry;
pub mod report;
pub mod rule_inference;
pub mod rules;
mod source_analysis;
pub mod spool;
pub mod staging;
pub mod template;
pub mod traversal;
pub mod work;
pub mod world;

/// Error type shared by the migration domain.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An underlying I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Input data does not satisfy the selected format profile.
    #[error("invalid world data: {0}")]
    InvalidData(String),
}

/// Result type shared by the migration domain.
pub type Result<T> = std::result::Result<T, Error>;
