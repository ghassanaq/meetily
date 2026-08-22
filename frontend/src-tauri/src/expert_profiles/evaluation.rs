use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::generation::{
    generate_profile_summary, ProfileGenerationError, ProfileGenerationRequest,
};
use super::hashing::{
    hash_capability_revision, hash_eval_plan, hash_model_binding, hash_profile_version, HashError,
};
use super::models::{
    AdjudicatorKind, EffectiveCapabilityRevision, EvalCase, EvalPlan, ExpertProfileVersion,
    HardAssertion, ModelGenerationBinding, SemanticAssertion,
};
use super::rendering::parse_profile_markdown;
use super::safety_gate::{safety_workload_for_playbook, SAFETY_GATE_VERSION};
use super::validation::{Validate, ValidationErrors};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvalTarget {
    Candidate,
    Baseline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticAdjudication {
    pub target: EvalTarget,
    pub case_id: String,
    pub repetition: u32,
    pub assertion_index: usize,
    pub score: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalRunOutcome {
    Pass,
    Fail,
    Rejected,
    Inconclusive,
    BaselineMissing,
}

impl EvalRunOutcome {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Rejected => "rejected",
            Self::Inconclusive => "inconclusive",
            Self::BaselineMissing => "baseline_missing",
        }
    }

    pub fn qualifies_for_activation(self) -> bool {
        matches!(self, Self::Pass | Self::BaselineMissing)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardAssertionResult {
    pub assertion: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticAssertionResult {
    pub assertion_index: usize,
    pub adjudicator: AdjudicatorKind,
    pub threshold: f64,
    pub score: Option<f64>,
    pub passed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvalRepetitionResult {
    pub target: EvalTarget,
    pub case_id: String,
    pub playbook_id: Uuid,
    pub repetition: u32,
    pub hard: Vec<HardAssertionResult>,
    pub semantic: Vec<SemanticAssertionResult>,
    pub output_markdown: Option<String>,
    pub generation_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvaluationReport {
    pub qualifying: bool,
    pub candidate_profile_version_hash: String,
    pub baseline_profile_version_hash: Option<String>,
    pub candidate_capability_revision: EffectiveCapabilityRevision,
    pub candidate_capability_hash: String,
    pub baseline_capability_hash: Option<String>,
    pub eval_plan_hash: String,
    pub model_binding_hash: String,
    pub safety_gate_version: String,
    pub repetitions: Vec<EvalRepetitionResult>,
    pub baseline_missing_playbooks: Vec<Uuid>,
    pub removed_playbooks: Vec<Uuid>,
    pub outcome: EvalRunOutcome,
    pub reasons: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error(transparent)]
    Validation(#[from] ValidationErrors),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error("candidate version hash does not match the supplied profile content")]
    CandidateDigestMismatch,
    #[error("baseline version hash does not match the supplied profile content")]
    BaselineDigestMismatch,
    #[error("evaluation plan hash does not match the source evaluation run")]
    EvalPlanDigestMismatch,
}

pub struct EvaluationRequest<'a> {
    pub profile_id: Uuid,
    pub candidate_profile_version_hash: &'a str,
    pub candidate: &'a ExpertProfileVersion,
    pub baseline_profile_version_hash: Option<&'a str>,
    pub baseline: Option<&'a ExpertProfileVersion>,
    pub plan: &'a EvalPlan,
    pub model_binding: &'a ModelGenerationBinding,
    pub qualifying: bool,
    pub confirmed_removed_playbooks: &'a [Uuid],
    pub adjudications: &'a [SemanticAdjudication],
    pub cancellation_token: Option<&'a CancellationToken>,
    pub progress: Option<&'a (dyn Fn(EvaluationProgress) + Sync)>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EvaluationProgress {
    pub completed_calls: usize,
    pub total_calls: usize,
    pub case_id: Option<String>,
    pub target: Option<EvalTarget>,
    pub repetition: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunStatus {
    Continue,
    Cancelled,
    ProviderCircuitOpen,
}

const MAX_CONSECUTIVE_GENERATION_ERRORS: usize = 3;

#[async_trait]
pub trait ProfileEvaluationBackend: Sync {
    async fn generate(
        &self,
        profile: &ExpertProfileVersion,
        playbook_id: Uuid,
        transcript: &str,
    ) -> Result<String, ProfileGenerationError>;
}

pub struct ProductionProfileEvaluationBackend<'a> {
    pub base_request: ProfileGenerationRequest<'a>,
}

#[async_trait]
impl ProfileEvaluationBackend for ProductionProfileEvaluationBackend<'_> {
    async fn generate(
        &self,
        profile: &ExpertProfileVersion,
        playbook_id: Uuid,
        transcript: &str,
    ) -> Result<String, ProfileGenerationError> {
        let base = &self.base_request;
        generate_profile_summary(ProfileGenerationRequest {
            client: base.client,
            provider: base.provider,
            model_name: base.model_name,
            api_key: base.api_key,
            transcript,
            additional_user_context: None,
            profile,
            playbook_id,
            token_threshold: base.token_threshold,
            ollama_endpoint: base.ollama_endpoint,
            custom_openai_endpoint: base.custom_openai_endpoint,
            max_tokens: base.max_tokens,
            temperature: base.temperature,
            top_p: base.top_p,
            app_data_dir: base.app_data_dir,
            cancellation_token: base.cancellation_token,
            summary_language: Some("en"),
            detected_transcript_language: Some("en"),
        })
        .await
        .map(|result| result.english_markdown)
    }
}

pub async fn run_evaluation(
    backend: &impl ProfileEvaluationBackend,
    request: EvaluationRequest<'_>,
) -> Result<EvaluationReport, EvaluationError> {
    request.candidate.validate()?;
    request.plan.validate_for_profile(request.candidate)?;
    if hash_profile_version(request.candidate)? != request.candidate_profile_version_hash {
        return Err(EvaluationError::CandidateDigestMismatch);
    }
    if let (Some(baseline), Some(expected_hash)) =
        (request.baseline, request.baseline_profile_version_hash)
    {
        baseline.validate()?;
        if hash_profile_version(baseline)? != expected_hash {
            return Err(EvaluationError::BaselineDigestMismatch);
        }
    }

    let model_binding_hash = hash_model_binding(request.model_binding)?;
    let eval_plan_hash = hash_eval_plan(request.plan)?;
    let candidate_revision = capability_revision(
        request.profile_id,
        request.candidate_profile_version_hash,
        request.candidate,
        &model_binding_hash,
        &eval_plan_hash,
    );
    let candidate_capability_hash = hash_capability_revision(&candidate_revision)?;
    let baseline_capability_hash = request
        .baseline
        .zip(request.baseline_profile_version_hash)
        .map(|(profile, version_hash)| {
            hash_capability_revision(&capability_revision(
                request.profile_id,
                version_hash,
                profile,
                &model_binding_hash,
                &eval_plan_hash,
            ))
        })
        .transpose()?;

    let candidate_ids: HashSet<Uuid> = request
        .candidate
        .playbooks
        .iter()
        .map(|playbook| playbook.id)
        .collect();
    let baseline_ids: HashSet<Uuid> = request
        .baseline
        .into_iter()
        .flat_map(|profile| profile.playbooks.iter().map(|playbook| playbook.id))
        .collect();
    let mut baseline_missing_playbooks: Vec<Uuid> =
        candidate_ids.difference(&baseline_ids).copied().collect();
    baseline_missing_playbooks.sort();
    let mut removed_playbooks: Vec<Uuid> =
        baseline_ids.difference(&candidate_ids).copied().collect();
    removed_playbooks.sort();

    let confirmed_removed: HashSet<Uuid> = request
        .confirmed_removed_playbooks
        .iter()
        .copied()
        .collect();
    let unconfirmed: Vec<Uuid> = removed_playbooks
        .iter()
        .copied()
        .filter(|id| !confirmed_removed.contains(id))
        .collect();

    let repetitions = if request.qualifying {
        request.plan.policy.activation_runs_per_case
    } else {
        1
    };
    let timeout = Duration::from_secs(u64::from(request.plan.policy.timeout_seconds));
    let adjudications = adjudication_map(request.adjudications);
    let mut results = Vec::new();

    let mut fixtures: HashMap<&str, &str> = request
        .plan
        .fixtures
        .iter()
        .map(|fixture| (fixture.id.as_str(), fixture.transcript_text.as_str()))
        .collect();
    let mut cases: Vec<EvalCase> = request.plan.cases.clone();
    let mut owned_safety_fixtures = Vec::new();
    for playbook in &request.candidate.playbooks {
        let safety = safety_workload_for_playbook(playbook.id);
        cases.extend(safety.cases);
        owned_safety_fixtures.extend(safety.fixtures);
    }
    for fixture in &owned_safety_fixtures {
        fixtures.insert(&fixture.id, &fixture.transcript_text);
    }

    let total_calls = cases
        .iter()
        .map(|case| {
            let targets = 1 + usize::from(baseline_ids.contains(&case.playbook_id));
            targets * repetitions as usize
        })
        .sum();
    let mut completed_calls = 0;
    let mut consecutive_generation_errors = 0;
    let mut stopped = RunStatus::Continue;
    report_progress(
        request.progress,
        EvaluationProgress {
            completed_calls,
            total_calls,
            case_id: None,
            target: None,
            repetition: None,
        },
    );

    for case in &cases {
        let transcript = fixtures
            .get(case.fixture_id.as_str())
            .expect("validated user cases and application safety cases have fixtures");
        stopped = run_case(
            backend,
            request.candidate,
            case,
            transcript,
            EvalTarget::Candidate,
            repetitions,
            timeout,
            request.plan.policy.semantic_min_score,
            &adjudications,
            &mut results,
            request.cancellation_token,
            request.progress,
            total_calls,
            &mut completed_calls,
            &mut consecutive_generation_errors,
        )
        .await;
        if stopped != RunStatus::Continue {
            break;
        }

        if baseline_ids.contains(&case.playbook_id) {
            stopped = run_case(
                backend,
                request
                    .baseline
                    .expect("baseline ID implies baseline profile"),
                case,
                transcript,
                EvalTarget::Baseline,
                repetitions,
                timeout,
                request.plan.policy.semantic_min_score,
                &adjudications,
                &mut results,
                request.cancellation_token,
                request.progress,
                total_calls,
                &mut completed_calls,
                &mut consecutive_generation_errors,
            )
            .await;
            if stopped != RunStatus::Continue {
                break;
            }
        }
    }

    let mut reasons = Vec::new();
    let any_generation_error = results
        .iter()
        .any(|result| result.generation_error.is_some());
    let any_hard_failure = results
        .iter()
        .filter(|result| result.target == EvalTarget::Candidate)
        .flat_map(|result| &result.hard)
        .any(|assertion| !assertion.passed);
    let any_semantic_unresolved = results
        .iter()
        .flat_map(|result| &result.semantic)
        .any(|assertion| assertion.passed.is_none());
    let any_semantic_failure = results
        .iter()
        .filter(|result| result.target == EvalTarget::Candidate)
        .flat_map(|result| &result.semantic)
        .any(|assertion| assertion.passed == Some(false));
    let inconsistent_semantics = semantic_repetitions_are_inconsistent(&results);
    let semantic_regression = has_semantic_regression(
        &results,
        request.plan.regression_policy.semantic_delta_floor,
    );

    if !unconfirmed.is_empty() {
        reasons.push(format!(
            "CAPABILITY_REMOVAL_UNCONFIRMED: {}",
            unconfirmed
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if any_generation_error {
        reasons.push("PROVIDER_UNAVAILABLE: one or more generation calls failed".to_string());
    }
    match stopped {
        RunStatus::Cancelled => reasons.push(
            "EVAL_CANCELLED: evaluation was cancelled; partial results were saved".to_string(),
        ),
        RunStatus::ProviderCircuitOpen => reasons.push(format!(
            "PROVIDER_UNAVAILABLE: stopped after {MAX_CONSECUTIVE_GENERATION_ERRORS} consecutive generation failures; partial results were saved"
        )),
        RunStatus::Continue => {}
    }
    if any_hard_failure {
        reasons.push("EVAL_FAILED: at least one hard assertion failed".to_string());
    }
    if any_semantic_unresolved {
        reasons.push("EVAL_INCONCLUSIVE: semantic adjudication is unresolved".to_string());
    }
    if inconsistent_semantics {
        reasons.push("EVAL_INCONCLUSIVE: semantic repetitions disagree".to_string());
    }
    if any_semantic_failure {
        reasons.push("EVAL_FAILED: at least one semantic assertion failed".to_string());
    }
    if semantic_regression {
        reasons
            .push("REGRESSION_DETECTED: semantic score fell below the allowed delta".to_string());
    }

    let outcome = if !unconfirmed.is_empty() {
        EvalRunOutcome::Rejected
    } else if stopped != RunStatus::Continue
        || any_generation_error
        || any_semantic_unresolved
        || inconsistent_semantics
    {
        EvalRunOutcome::Inconclusive
    } else if any_hard_failure || any_semantic_failure || semantic_regression {
        EvalRunOutcome::Fail
    } else if request.baseline.is_none() {
        EvalRunOutcome::BaselineMissing
    } else {
        EvalRunOutcome::Pass
    };

    Ok(EvaluationReport {
        qualifying: request.qualifying,
        candidate_profile_version_hash: request.candidate_profile_version_hash.to_string(),
        baseline_profile_version_hash: request
            .baseline_profile_version_hash
            .map(ToString::to_string),
        candidate_capability_revision: candidate_revision,
        candidate_capability_hash,
        baseline_capability_hash,
        eval_plan_hash,
        model_binding_hash,
        safety_gate_version: SAFETY_GATE_VERSION.to_string(),
        repetitions: results,
        baseline_missing_playbooks,
        removed_playbooks,
        outcome,
        reasons,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_case(
    backend: &impl ProfileEvaluationBackend,
    profile: &ExpertProfileVersion,
    case: &EvalCase,
    transcript: &str,
    target: EvalTarget,
    repetitions: u32,
    timeout: Duration,
    semantic_min_score: f64,
    adjudications: &HashMap<(EvalTarget, &str, u32, usize), f64>,
    results: &mut Vec<EvalRepetitionResult>,
    cancellation_token: Option<&CancellationToken>,
    progress: Option<&(dyn Fn(EvaluationProgress) + Sync)>,
    total_calls: usize,
    completed_calls: &mut usize,
    consecutive_generation_errors: &mut usize,
) -> RunStatus {
    for repetition in 0..repetitions {
        if cancellation_token.is_some_and(CancellationToken::is_cancelled) {
            return RunStatus::Cancelled;
        }
        let generation = tokio::time::timeout(
            timeout,
            backend.generate(profile, case.playbook_id, transcript),
        );
        let generated = match cancellation_token {
            Some(token) => tokio::select! {
                _ = token.cancelled() => return RunStatus::Cancelled,
                result = generation => result,
            },
            None => generation.await,
        };
        let (hard, semantic, output_markdown, generation_error) = match generated {
            Ok(Ok(markdown)) => {
                let hard = evaluate_hard(profile, case, &markdown);
                let semantic =
                    evaluate_semantic(case, target, repetition, adjudications, semantic_min_score);
                (hard, semantic, Some(markdown), None)
            }
            Ok(Err(error)) => (Vec::new(), Vec::new(), None, Some(error.to_string())),
            Err(_) => (
                Vec::new(),
                Vec::new(),
                None,
                Some("evaluation generation timed out".to_string()),
            ),
        };
        results.push(EvalRepetitionResult {
            target,
            case_id: case.id.clone(),
            playbook_id: case.playbook_id,
            repetition,
            hard,
            semantic,
            output_markdown,
            generation_error,
        });
        *completed_calls += 1;
        let failed = results
            .last()
            .is_some_and(|result| result.generation_error.is_some());
        if failed {
            *consecutive_generation_errors += 1;
        } else {
            *consecutive_generation_errors = 0;
        }
        report_progress(
            progress,
            EvaluationProgress {
                completed_calls: *completed_calls,
                total_calls,
                case_id: Some(case.id.clone()),
                target: Some(target),
                repetition: Some(repetition),
            },
        );
        if *consecutive_generation_errors >= MAX_CONSECUTIVE_GENERATION_ERRORS {
            return RunStatus::ProviderCircuitOpen;
        }
    }
    RunStatus::Continue
}

fn report_progress(
    progress: Option<&(dyn Fn(EvaluationProgress) + Sync)>,
    update: EvaluationProgress,
) {
    if let Some(progress) = progress {
        progress(update);
    }
}

fn evaluate_hard(
    profile: &ExpertProfileVersion,
    case: &EvalCase,
    markdown: &str,
) -> Vec<HardAssertionResult> {
    let parsed = parse_profile_markdown(profile, case.playbook_id, markdown);
    case.assertions
        .hard
        .iter()
        .map(|assertion| match assertion {
            HardAssertion::SchemaCompliance => match &parsed {
                Ok(parsed) => HardAssertionResult {
                    assertion: "schema_compliance".to_string(),
                    passed: parsed.is_schema_compliant(),
                    detail: parsed.errors.join("; "),
                },
                Err(error) => HardAssertionResult {
                    assertion: "schema_compliance".to_string(),
                    passed: false,
                    detail: error.to_string(),
                },
            },
            HardAssertion::SectionPresent { section_id } => {
                let passed = parsed.as_ref().ok().is_some_and(|parsed| {
                    parsed
                        .sections
                        .get(section_id)
                        .is_some_and(|body| !body.trim().is_empty())
                });
                HardAssertionResult {
                    assertion: format!("section_present:{section_id}"),
                    passed,
                    detail: if passed {
                        String::new()
                    } else {
                        format!("section '{section_id}' is missing or empty")
                    },
                }
            }
            HardAssertion::LiteralPresent { value } => HardAssertionResult {
                assertion: format!("literal_present:{value}"),
                passed: markdown.contains(value),
                detail: format!("expected literal '{value}' to be present"),
            },
            HardAssertion::LiteralAbsent { value } => HardAssertionResult {
                assertion: format!("literal_absent:{value}"),
                passed: !markdown.contains(value),
                detail: format!("expected literal '{value}' to be absent"),
            },
        })
        .collect()
}

fn evaluate_semantic(
    case: &EvalCase,
    target: EvalTarget,
    repetition: u32,
    adjudications: &HashMap<(EvalTarget, &str, u32, usize), f64>,
    semantic_min_score: f64,
) -> Vec<SemanticAssertionResult> {
    case.assertions
        .semantic
        .iter()
        .enumerate()
        .map(|(index, assertion)| match assertion {
            SemanticAssertion::Rubric {
                adjudicator,
                threshold,
                ..
            } => {
                let score = match adjudicator {
                    AdjudicatorKind::Human => adjudications
                        .get(&(target, case.id.as_str(), repetition, index))
                        .copied(),
                    AdjudicatorKind::Model => None,
                };
                let effective_threshold = threshold.max(semantic_min_score);
                SemanticAssertionResult {
                    assertion_index: index,
                    adjudicator: *adjudicator,
                    threshold: effective_threshold,
                    score,
                    passed: score.map(|score| score >= effective_threshold),
                }
            }
        })
        .collect()
}

fn adjudication_map<'a>(
    adjudications: &'a [SemanticAdjudication],
) -> HashMap<(EvalTarget, &'a str, u32, usize), f64> {
    adjudications
        .iter()
        .filter(|item| item.score.is_finite() && (0.0..=1.0).contains(&item.score))
        .map(|item| {
            (
                (
                    item.target,
                    item.case_id.as_str(),
                    item.repetition,
                    item.assertion_index,
                ),
                item.score,
            )
        })
        .collect()
}

fn semantic_repetitions_are_inconsistent(results: &[EvalRepetitionResult]) -> bool {
    let mut states: HashMap<(EvalTarget, &str, usize), HashSet<bool>> = HashMap::new();
    for result in results {
        for semantic in &result.semantic {
            if let Some(passed) = semantic.passed {
                states
                    .entry((
                        result.target,
                        result.case_id.as_str(),
                        semantic.assertion_index,
                    ))
                    .or_default()
                    .insert(passed);
            }
        }
    }
    states.values().any(|states| states.len() > 1)
}

fn has_semantic_regression(results: &[EvalRepetitionResult], delta_floor: f64) -> bool {
    let mut scores: HashMap<(&str, usize), (Vec<f64>, Vec<f64>)> = HashMap::new();
    for result in results {
        for semantic in &result.semantic {
            if let Some(score) = semantic.score {
                let entry = scores
                    .entry((result.case_id.as_str(), semantic.assertion_index))
                    .or_default();
                match result.target {
                    EvalTarget::Candidate => entry.0.push(score),
                    EvalTarget::Baseline => entry.1.push(score),
                }
            }
        }
    }

    scores.values().any(|(candidate, baseline)| {
        if candidate.is_empty() || baseline.is_empty() {
            return false;
        }
        let candidate_mean = candidate.iter().sum::<f64>() / candidate.len() as f64;
        let baseline_mean = baseline.iter().sum::<f64>() / baseline.len() as f64;
        candidate_mean - baseline_mean < delta_floor
    })
}

pub fn adjudicate_evaluation_report(
    source: &EvaluationReport,
    plan: &EvalPlan,
    adjudications: &[SemanticAdjudication],
) -> Result<EvaluationReport, EvaluationError> {
    plan.validate()?;
    if hash_eval_plan(plan)? != source.eval_plan_hash {
        return Err(EvaluationError::EvalPlanDigestMismatch);
    }
    let mut report = source.clone();
    let adjudications = adjudication_map(adjudications);
    for repetition in &mut report.repetitions {
        let Some(case) = plan.cases.iter().find(|case| case.id == repetition.case_id) else {
            continue;
        };
        repetition.semantic = evaluate_semantic(
            case,
            repetition.target,
            repetition.repetition,
            &adjudications,
            plan.policy.semantic_min_score,
        );
    }

    let unconfirmed_removal = report
        .reasons
        .iter()
        .any(|reason| reason.starts_with("CAPABILITY_REMOVAL_UNCONFIRMED"));
    let generation_error = report
        .repetitions
        .iter()
        .any(|result| result.generation_error.is_some());
    let hard_failure = report
        .repetitions
        .iter()
        .filter(|result| result.target == EvalTarget::Candidate)
        .flat_map(|result| &result.hard)
        .any(|assertion| !assertion.passed);
    let semantic_unresolved = report
        .repetitions
        .iter()
        .flat_map(|result| &result.semantic)
        .any(|assertion| assertion.passed.is_none());
    let semantic_failure = report
        .repetitions
        .iter()
        .filter(|result| result.target == EvalTarget::Candidate)
        .flat_map(|result| &result.semantic)
        .any(|assertion| assertion.passed == Some(false));
    let inconsistent = semantic_repetitions_are_inconsistent(&report.repetitions);
    let regression = has_semantic_regression(
        &report.repetitions,
        plan.regression_policy.semantic_delta_floor,
    );

    report.reasons.clear();
    if !report.qualifying {
        report
            .reasons
            .push("EVAL_FAILED: preview runs cannot qualify for activation".to_string());
    }
    if unconfirmed_removal {
        report
            .reasons
            .push("CAPABILITY_REMOVAL_UNCONFIRMED".to_string());
    }
    if generation_error {
        report
            .reasons
            .push("PROVIDER_UNAVAILABLE: one or more generation calls failed".to_string());
    }
    if hard_failure {
        report
            .reasons
            .push("EVAL_FAILED: at least one hard assertion failed".to_string());
    }
    if semantic_unresolved {
        report
            .reasons
            .push("EVAL_INCONCLUSIVE: semantic adjudication is unresolved".to_string());
    }
    if inconsistent {
        report
            .reasons
            .push("EVAL_INCONCLUSIVE: semantic repetitions disagree".to_string());
    }
    if semantic_failure {
        report
            .reasons
            .push("EVAL_FAILED: at least one semantic assertion failed".to_string());
    }
    if regression {
        report
            .reasons
            .push("REGRESSION_DETECTED: semantic score fell below the allowed delta".to_string());
    }

    report.outcome = if !report.qualifying || unconfirmed_removal {
        EvalRunOutcome::Rejected
    } else if generation_error || semantic_unresolved || inconsistent {
        EvalRunOutcome::Inconclusive
    } else if hard_failure || semantic_failure || regression {
        EvalRunOutcome::Fail
    } else if report.baseline_profile_version_hash.is_none() {
        EvalRunOutcome::BaselineMissing
    } else {
        EvalRunOutcome::Pass
    };
    Ok(report)
}

fn capability_revision(
    profile_id: Uuid,
    profile_version_hash: &str,
    profile: &ExpertProfileVersion,
    model_binding_hash: &str,
    eval_plan_hash: &str,
) -> EffectiveCapabilityRevision {
    let mut playbook_ids: Vec<Uuid> = profile
        .playbooks
        .iter()
        .map(|playbook| playbook.id)
        .collect();
    playbook_ids.sort();
    EffectiveCapabilityRevision {
        profile_id,
        profile_version_hash: profile_version_hash.to_string(),
        playbook_ids,
        model_binding_hash: model_binding_hash.to_string(),
        eval_plan_hash: eval_plan_hash.to_string(),
        safety_gate_version: SAFETY_GATE_VERSION.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expert_profiles::hashing::{hash_profile_version, prompt_renderer_hash};
    use crate::expert_profiles::models::GenerationParameters;
    use crate::expert_profiles::safety_gate::INJECTION_CANARY;
    use crate::expert_profiles::tests::{sample_eval_plan, sample_profile};

    struct DeterministicBackend {
        echo_transcript: bool,
        fail: bool,
    }

    #[async_trait]
    impl ProfileEvaluationBackend for DeterministicBackend {
        async fn generate(
            &self,
            profile: &ExpertProfileVersion,
            playbook_id: Uuid,
            transcript: &str,
        ) -> Result<String, ProfileGenerationError> {
            if self.fail {
                return Err(ProfileGenerationError::Provider(
                    "provider temporarily unavailable".to_string(),
                ));
            }
            let playbook = profile
                .playbooks
                .iter()
                .find(|playbook| playbook.id == playbook_id)
                .unwrap();
            let evidence = if self.echo_transcript {
                transcript.to_string()
            } else {
                transcript.replace(INJECTION_CANARY, "[ignored instruction]")
            };
            let mut markdown = "# Evaluation report\n\n".to_string();
            for section in profile
                .output_contract
                .sections
                .iter()
                .chain(playbook.sections.iter())
            {
                markdown.push_str(&format!("**{}**\n\n- {}\n\n", section.title, evidence));
            }
            Ok(markdown)
        }
    }

    fn binding() -> ModelGenerationBinding {
        ModelGenerationBinding {
            provider: "custom-openai".to_string(),
            model: "test-model".to_string(),
            model_artifact_hash: None,
            endpoint_fingerprint: Some("sha256:test-endpoint".to_string()),
            generation_parameters: GenerationParameters {
                temperature: 0.0,
                max_tokens: 1024,
            },
            prompt_renderer_hash: prompt_renderer_hash(),
            output_parser_version: super::super::rendering::OUTPUT_PARSER_VERSION,
        }
    }

    fn adjudications(
        plan: &EvalPlan,
        target: EvalTarget,
        scores: &[f64],
    ) -> Vec<SemanticAdjudication> {
        scores
            .iter()
            .enumerate()
            .map(|(repetition, score)| SemanticAdjudication {
                target,
                case_id: plan.cases[0].id.clone(),
                repetition: repetition as u32,
                assertion_index: 0,
                score: *score,
            })
            .collect()
    }

    async fn evaluate(
        backend: &DeterministicBackend,
        candidate: &ExpertProfileVersion,
        baseline: Option<&ExpertProfileVersion>,
        plan: &EvalPlan,
        scores: &[SemanticAdjudication],
        confirmed_removed: &[Uuid],
    ) -> EvaluationReport {
        let candidate_hash = hash_profile_version(candidate).unwrap();
        let baseline_hash = baseline.map(|profile| hash_profile_version(profile).unwrap());
        run_evaluation(
            backend,
            EvaluationRequest {
                profile_id: Uuid::new_v4(),
                candidate_profile_version_hash: &candidate_hash,
                candidate,
                baseline_profile_version_hash: baseline_hash.as_deref(),
                baseline,
                plan,
                model_binding: &binding(),
                qualifying: true,
                confirmed_removed_playbooks: confirmed_removed,
                adjudications: scores,
                cancellation_token: None,
                progress: None,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn first_activation_passes_only_after_all_repetitions_and_safety_cases_pass() {
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let scores = adjudications(&plan, EvalTarget::Candidate, &[0.9, 0.9]);
        let report = evaluate(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            &profile,
            None,
            &plan,
            &scores,
            &[],
        )
        .await;

        assert_eq!(report.outcome, EvalRunOutcome::BaselineMissing);
        assert!(report.outcome.qualifies_for_activation());
        assert_eq!(report.baseline_missing_playbooks, [profile.playbooks[0].id]);
        assert_eq!(report.repetitions.len(), 8);
        assert!(report.reasons.is_empty());
    }

    #[tokio::test]
    async fn unresolved_human_rubric_is_inconclusive_and_fails_closed() {
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let report = evaluate(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            &profile,
            None,
            &plan,
            &[],
            &[],
        )
        .await;

        assert_eq!(report.outcome, EvalRunOutcome::Inconclusive);
        assert!(!report.outcome.qualifies_for_activation());
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("semantic adjudication is unresolved")));
    }

    #[tokio::test]
    async fn human_adjudication_reuses_the_pinned_outputs_without_regeneration() {
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let report = evaluate(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            &profile,
            None,
            &plan,
            &[],
            &[],
        )
        .await;
        let pinned_outputs: Vec<_> = report
            .repetitions
            .iter()
            .map(|result| result.output_markdown.clone())
            .collect();
        let scores = adjudications(&plan, EvalTarget::Candidate, &[0.9, 0.9]);

        let adjudicated = adjudicate_evaluation_report(&report, &plan, &scores).unwrap();

        assert_eq!(adjudicated.outcome, EvalRunOutcome::BaselineMissing);
        assert_eq!(
            adjudicated
                .repetitions
                .iter()
                .map(|result| result.output_markdown.clone())
                .collect::<Vec<_>>(),
            pinned_outputs
        );
    }

    #[tokio::test]
    async fn application_injection_fixture_cannot_be_removed_by_the_user_plan() {
        let profile = sample_profile();
        let mut plan = sample_eval_plan(&profile);
        plan.cases[0].assertions.semantic.clear();
        let report = evaluate(
            &DeterministicBackend {
                echo_transcript: true,
                fail: false,
            },
            &profile,
            None,
            &plan,
            &[],
            &[],
        )
        .await;

        assert_eq!(report.outcome, EvalRunOutcome::Fail);
        assert!(report.repetitions.iter().any(|result| {
            result.hard.iter().any(|assertion| {
                assertion.assertion.contains(INJECTION_CANARY) && !assertion.passed
            })
        }));
    }

    #[tokio::test]
    async fn provider_failure_is_inconclusive_and_does_not_qualify() {
        let profile = sample_profile();
        let mut plan = sample_eval_plan(&profile);
        plan.cases[0].assertions.semantic.clear();
        let report = evaluate(
            &DeterministicBackend {
                echo_transcript: false,
                fail: true,
            },
            &profile,
            None,
            &plan,
            &[],
            &[],
        )
        .await;

        assert_eq!(report.outcome, EvalRunOutcome::Inconclusive);
        assert_eq!(report.repetitions.len(), MAX_CONSECUTIVE_GENERATION_ERRORS);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.starts_with("PROVIDER_UNAVAILABLE")));
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.contains("partial results were saved")));
    }

    #[tokio::test]
    async fn cancelled_evaluation_returns_a_persistable_partial_report() {
        let profile = sample_profile();
        let mut plan = sample_eval_plan(&profile);
        plan.cases[0].assertions.semantic.clear();
        let candidate_hash = hash_profile_version(&profile).unwrap();
        let token = CancellationToken::new();
        token.cancel();

        let report = run_evaluation(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            EvaluationRequest {
                profile_id: Uuid::new_v4(),
                candidate_profile_version_hash: &candidate_hash,
                candidate: &profile,
                baseline_profile_version_hash: None,
                baseline: None,
                plan: &plan,
                model_binding: &binding(),
                qualifying: true,
                confirmed_removed_playbooks: &[],
                adjudications: &[],
                cancellation_token: Some(&token),
                progress: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(report.outcome, EvalRunOutcome::Inconclusive);
        assert!(report.repetitions.is_empty());
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.starts_with("EVAL_CANCELLED")));
    }

    #[tokio::test]
    async fn progress_reports_the_total_and_every_completed_provider_call() {
        let profile = sample_profile();
        let mut plan = sample_eval_plan(&profile);
        plan.cases[0].assertions.semantic.clear();
        let candidate_hash = hash_profile_version(&profile).unwrap();
        let updates = std::sync::Mutex::new(Vec::new());
        let progress = |update| updates.lock().unwrap().push(update);

        let report = run_evaluation(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            EvaluationRequest {
                profile_id: Uuid::new_v4(),
                candidate_profile_version_hash: &candidate_hash,
                candidate: &profile,
                baseline_profile_version_hash: None,
                baseline: None,
                plan: &plan,
                model_binding: &binding(),
                qualifying: true,
                confirmed_removed_playbooks: &[],
                adjudications: &[],
                cancellation_token: None,
                progress: Some(&progress),
            },
        )
        .await
        .unwrap();

        let updates = updates.into_inner().unwrap();
        let total = report.repetitions.len();
        assert_eq!(updates.len(), total + 1);
        assert_eq!(updates[0].completed_calls, 0);
        assert_eq!(updates[0].total_calls, total);
        assert_eq!(updates.last().unwrap().completed_calls, total);
        assert_eq!(updates.last().unwrap().total_calls, total);
    }

    #[tokio::test]
    async fn removing_an_active_playbook_requires_explicit_confirmation() {
        let candidate = sample_profile();
        let mut baseline = candidate.clone();
        let mut removed = baseline.playbooks[0].clone();
        removed.id = Uuid::new_v4();
        removed.name = "Removed planning playbook".to_string();
        baseline.playbooks.push(removed.clone());
        let mut plan = sample_eval_plan(&candidate);
        plan.cases[0].assertions.semantic.clear();

        let rejected = evaluate(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            &candidate,
            Some(&baseline),
            &plan,
            &[],
            &[],
        )
        .await;
        assert_eq!(rejected.outcome, EvalRunOutcome::Rejected);

        let accepted = evaluate(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            &candidate,
            Some(&baseline),
            &plan,
            &[],
            &[removed.id],
        )
        .await;
        assert_eq!(accepted.outcome, EvalRunOutcome::Pass);
    }

    #[tokio::test]
    async fn semantic_regression_beyond_the_plan_floor_blocks_activation() {
        let profile = sample_profile();
        let mut plan = sample_eval_plan(&profile);
        plan.policy.semantic_min_score = 0.4;
        let SemanticAssertion::Rubric { threshold, .. } = &mut plan.cases[0].assertions.semantic[0];
        *threshold = 0.4;
        let mut scores = adjudications(&plan, EvalTarget::Candidate, &[0.9, 0.9]);
        scores.extend(adjudications(&plan, EvalTarget::Baseline, &[1.0, 1.0]));
        let report = evaluate(
            &DeterministicBackend {
                echo_transcript: false,
                fail: false,
            },
            &profile,
            Some(&profile),
            &plan,
            &scores,
            &[],
        )
        .await;

        assert_eq!(report.outcome, EvalRunOutcome::Fail);
        assert!(report
            .reasons
            .iter()
            .any(|reason| reason.starts_with("REGRESSION_DETECTED")));
    }
}
