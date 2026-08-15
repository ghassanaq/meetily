use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const EXPERT_PROFILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExpertProfileVersion {
    pub schema_version: u32,
    pub identity: ProfileIdentity,
    pub objectives: Vec<String>,
    pub perspective: String,
    pub style: ProfileStyle,
    pub boundaries: ProfileBoundaries,
    pub retrieval_policy: RetrievalPolicy,
    pub output_contract: OutputContract,
    pub playbooks: Vec<MeetingPlaybook>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileIdentity {
    pub name: String,
    pub description: String,
    pub expertise: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileStyle {
    pub tone: String,
    pub verbosity: String,
    pub language: String,
    pub format: ProfileOutputFormat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileOutputFormat {
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileBoundaries {
    pub in_scope: Vec<String>,
    pub out_of_scope: Vec<String>,
    pub abstain_when: Vec<String>,
    pub escalation_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetrievalPolicy {
    pub mode: RetrievalMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    TranscriptOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    pub title_required: bool,
    pub sections: Vec<OutputSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputSection {
    pub id: String,
    pub title: String,
    pub instruction: String,
    pub format: SectionFormat,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SectionFormat {
    Paragraph,
    List,
    String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MeetingPlaybook {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub objective: String,
    pub sections: Vec<OutputSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvalPlan {
    pub schema_version: u32,
    pub fixtures: Vec<EvalFixture>,
    pub cases: Vec<EvalCase>,
    pub policy: EvalPolicy,
    pub regression_policy: RegressionPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvalFixture {
    pub id: String,
    pub content_hash: String,
    pub source: String,
    pub transcript_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvalCase {
    pub id: String,
    pub fixture_id: String,
    pub playbook_id: Uuid,
    pub assertions: EvalAssertions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvalAssertions {
    pub hard: Vec<HardAssertion>,
    pub semantic: Vec<SemanticAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HardAssertion {
    SchemaCompliance,
    SectionPresent { section_id: String },
    LiteralPresent { value: String },
    LiteralAbsent { value: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticAssertion {
    Rubric {
        question: String,
        adjudicator: AdjudicatorKind,
        threshold: f64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdjudicatorKind {
    Human,
    Model,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvalPolicy {
    pub activation_runs_per_case: u32,
    pub all_hard_runs_must_pass: bool,
    pub semantic_min_score: f64,
    pub timeout_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RegressionPolicy {
    pub hard_rule: HardRegressionRule,
    pub semantic_delta_floor: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HardRegressionRule {
    NoNewHardFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModelGenerationBinding {
    pub provider: String,
    pub model: String,
    pub model_artifact_hash: Option<String>,
    pub endpoint_fingerprint: Option<String>,
    pub generation_parameters: GenerationParameters,
    pub prompt_renderer_hash: String,
    pub output_parser_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GenerationParameters {
    pub temperature: f64,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EffectiveCapabilityRevision {
    pub profile_id: Uuid,
    pub profile_version_hash: String,
    pub playbook_ids: Vec<Uuid>,
    pub model_binding_hash: String,
    pub eval_plan_hash: String,
    pub safety_gate_version: String,
}
