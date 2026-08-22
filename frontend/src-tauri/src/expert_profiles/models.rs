use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const EXPERT_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const EVAL_PLAN_SCHEMA_VERSION: u32 = 2;

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
    #[serde(default)]
    pub suite: EvalSuite,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_shape: Option<AnswerShape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_contracts: Vec<EvidenceContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_records: Vec<EvalEvidenceRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_elements: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_expansions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applicability: Option<MandatoryApplicability>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvalSuite {
    Safety,
    #[default]
    Depth,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AnswerShape {
    StrategicImplementation,
    DirectFactualCommitment,
    UrgentOperational,
    EthicalScenario,
    GovernanceSafeguardingFinancial,
    ExternalPartnership,
    CareerSuitability,
    CapabilityGap,
    BehavioralFailure,
    ComparativeClosing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceContract {
    DocumentedOnly,
    ProspectiveAllowed,
    BoundaryThenProspective,
    ConditionalCommitment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvalEvidenceRecord {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DimensionApplicability {
    Applicable,
    NotApplicable,
    Expected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MandatoryApplicability {
    pub grounding: DimensionApplicability,
    pub authority: DimensionApplicability,
    pub past_vs_prospective: DimensionApplicability,
    pub directness: DimensionApplicability,
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
    DimensionRubric {
        dimension: EvaluationDimension,
        applicability: DimensionApplicability,
        question: String,
        adjudicator: AdjudicatorKind,
        threshold: f64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationDimension {
    Grounding,
    Authority,
    PastVsProspective,
    Directness,
    Depth,
    Concision,
}

impl EvaluationDimension {
    pub fn is_mandatory(self) -> bool {
        matches!(
            self,
            Self::Grounding | Self::Authority | Self::PastVsProspective | Self::Directness
        )
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_configuration_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_revision: Option<i64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    pub max_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_suite_hash: Option<String>,
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use crate::expert_profiles::hashing::hash_model_binding;

    #[test]
    fn legacy_binding_payload_round_trips_without_new_empty_fields() {
        let legacy = serde_json::json!({
            "provider": "custom-openai",
            "model": "model-a",
            "model_artifact_hash": null,
            "endpoint_fingerprint": "sha256:endpoint",
            "generation_parameters": {
                "temperature": 0.0,
                "max_tokens": 2048
            },
            "prompt_renderer_hash": "sha256:renderer",
            "output_parser_version": 1
        });
        let binding: ModelGenerationBinding = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(serde_json::to_value(binding).unwrap(), legacy);
    }

    #[test]
    fn managed_provider_revision_changes_the_binding_hash() {
        let mut binding = ModelGenerationBinding {
            provider: "openai".to_string(),
            model: "gpt-test".to_string(),
            provider_record_id: Some(Uuid::nil().to_string()),
            provider_configuration_hash: Some("config-a".to_string()),
            credential_revision: Some(1),
            model_artifact_hash: None,
            endpoint_fingerprint: Some("sha256:endpoint".to_string()),
            generation_parameters: GenerationParameters {
                temperature: 0.0,
                top_p: None,
                max_tokens: 2048,
                reasoning_effort: None,
            },
            prompt_renderer_hash: "sha256:renderer".to_string(),
            output_parser_version: 1,
        };
        let first = hash_model_binding(&binding).unwrap();
        binding.credential_revision = Some(2);
        binding.provider_configuration_hash = Some("config-b".to_string());
        assert_ne!(first, hash_model_binding(&binding).unwrap());
    }
}
