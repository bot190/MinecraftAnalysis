//! Shared conversion errors for template-based transformations.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("template transformation failed: {source}")]
    Template {
        #[source]
        source: Box<crate::rules::Error>,
    },
    #[error("object {identity} has no target mapping and no explicit loss policy")]
    Unresolved { identity: String },
}
