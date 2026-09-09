//! Target-aware diagnostics for one selected world coordinate.

use std::path::Path;

/// Explain supported objects owned by one world-global coordinate.
///
/// # Errors
///
/// Returns contextual dimension, region, chunk, decode, traversal, nested-item,
/// or rule-assessment failures.
pub fn at_coordinate_with_progress(
    source: &Path,
    source_catalog: &crate::registry::RegistryCatalog,
    target_catalog: &crate::registry::RegistryCatalog,
    rules: &crate::rules::LoadedRules,
    dimension: &crate::world::DimensionId,
    block: [i32; 3],
    progress: &dyn crate::progress::ProgressObserver,
) -> crate::preflight::Result<Vec<crate::report::ObjectRecord>> {
    crate::preflight::explain_at_coordinate_with_progress(
        source,
        source_catalog,
        target_catalog,
        rules,
        dimension,
        block,
        progress,
    )
}
