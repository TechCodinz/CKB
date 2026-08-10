// Module bridge allowing the high-fidelity extractor to stay sibling-scoped
// while being compiled as part of the Deep Causality bundle surface.
pub mod deep_causality {
    pub use crate::analysis::deep_causality::*;
}
pub mod deep_causality_extractors {
    pub use crate::analysis::deep_causality_extractors::*;
}

#[path = "deep_causality_artifacts_v2.rs"]
mod implementation;

pub use implementation::enrich_deep_artifact_semantics;
