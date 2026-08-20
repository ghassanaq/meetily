//! Declarative Expert Profile and Meeting Playbook domain contracts.
//!
//! Profile schemas intentionally contain no filesystem, network, shell, tool,
//! permission, or executable capability fields. The command and generation
//! adapters can only route validated profile data into the existing summary
//! provider path and persist the returned Markdown as inert data.

pub mod bundle;
pub mod commands;
pub mod evaluation;
pub mod generation;
pub mod hashing;
pub mod models;
pub mod presets;
pub mod rendering;
pub mod safety_gate;
pub mod validation;

pub use bundle::{
    export_bundle, import_bundle, parse_bundle_json, BundleError, ExpertProfileBundle,
    ImportIdentityMode, ImportResult,
};

pub use evaluation::{
    adjudicate_evaluation_report, run_evaluation, EvalRunOutcome, EvaluationReport,
    EvaluationRequest, ProductionProfileEvaluationBackend, ProfileEvaluationBackend,
    SemanticAdjudication,
};
pub use generation::{generate_profile_summary, ProfileGenerationRequest, ProfileGenerationResult};
pub use hashing::{
    hash_capability_revision, hash_eval_plan, hash_fixture_text, hash_model_binding,
    hash_profile_version, prompt_renderer_hash,
};
pub use models::*;
pub use rendering::{
    build_profile_render_spec, parse_profile_markdown, ProfileRenderError, ProfileRenderSpec,
    OUTPUT_PARSER_VERSION, PROMPT_RENDERER_VERSION,
};
pub use validation::{
    parse_eval_plan_json, parse_profile_json, Validate, ValidationError, ValidationErrorCode,
    ValidationErrors,
};

#[cfg(test)]
pub(crate) mod tests;
