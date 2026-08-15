use serde::Serialize;
use std::collections::{HashMap, HashSet};
use thiserror::Error;
use uuid::Uuid;

use super::hashing::hash_fixture_text;
use super::models::{
    EvalPlan, ExpertProfileVersion, HardAssertion, OutputSection, SemanticAssertion,
    EXPERT_PROFILE_SCHEMA_VERSION,
};

const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_STRING_BYTES: usize = 16 * 1024;
const MAX_FIXTURE_BYTES: usize = 200 * 1024;
const MAX_OBJECTIVES: usize = 32;
const MAX_PLAYBOOKS: usize = 32;
const MAX_SECTIONS: usize = 32;
const MAX_CASES: usize = 64;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationErrorCode {
    EmptyEvalPlan,
    UnknownField,
    InvalidPlaybook,
    SchemaMismatch,
    LimitExceeded,
    InvalidReference,
    DuplicateValue,
    DigestMismatch,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidationError {
    pub code: ValidationErrorCode,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Error)]
#[error("expert profile validation failed")]
pub struct ValidationErrors(pub Vec<ValidationError>);

pub trait Validate {
    fn validate(&self) -> Result<(), ValidationErrors>;
}

impl Validate for ExpertProfileVersion {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = Vec::new();

        if self.schema_version != EXPERT_PROFILE_SCHEMA_VERSION {
            push_error(
                &mut errors,
                ValidationErrorCode::SchemaMismatch,
                "$.schema_version",
                format!(
                    "expected schema version {EXPERT_PROFILE_SCHEMA_VERSION}, got {}",
                    self.schema_version
                ),
            );
        }

        validate_text(&mut errors, "$.identity.name", &self.identity.name);
        validate_text(
            &mut errors,
            "$.identity.description",
            &self.identity.description,
        );
        validate_non_empty_text_list(
            &mut errors,
            "$.identity.expertise",
            &self.identity.expertise,
            MAX_OBJECTIVES,
        );
        validate_non_empty_text_list(
            &mut errors,
            "$.objectives",
            &self.objectives,
            MAX_OBJECTIVES,
        );
        validate_text(&mut errors, "$.perspective", &self.perspective);
        validate_text(&mut errors, "$.style.tone", &self.style.tone);
        validate_text(&mut errors, "$.style.verbosity", &self.style.verbosity);
        validate_language(&mut errors, "$.style.language", &self.style.language);
        validate_text_list(
            &mut errors,
            "$.boundaries.in_scope",
            &self.boundaries.in_scope,
            MAX_OBJECTIVES,
        );
        validate_text_list(
            &mut errors,
            "$.boundaries.out_of_scope",
            &self.boundaries.out_of_scope,
            MAX_OBJECTIVES,
        );
        validate_text_list(
            &mut errors,
            "$.boundaries.abstain_when",
            &self.boundaries.abstain_when,
            MAX_OBJECTIVES,
        );
        validate_text(
            &mut errors,
            "$.boundaries.escalation_policy",
            &self.boundaries.escalation_policy,
        );

        validate_sections(
            &mut errors,
            "$.output_contract.sections",
            &self.output_contract.sections,
            true,
        );

        if self.playbooks.is_empty() {
            push_error(
                &mut errors,
                ValidationErrorCode::InvalidPlaybook,
                "$.playbooks",
                "at least one embedded playbook is required",
            );
        }
        if self.playbooks.len() > MAX_PLAYBOOKS {
            push_limit_error(&mut errors, "$.playbooks", MAX_PLAYBOOKS);
        }

        let mut playbook_ids = HashSet::new();
        for (index, playbook) in self.playbooks.iter().enumerate() {
            let path = format!("$.playbooks[{index}]");
            if !playbook_ids.insert(playbook.id) {
                push_error(
                    &mut errors,
                    ValidationErrorCode::DuplicateValue,
                    format!("{path}.id"),
                    "playbook IDs must be unique within a profile",
                );
            }

            validate_text(&mut errors, &format!("{path}.name"), &playbook.name);
            validate_text(
                &mut errors,
                &format!("{path}.description"),
                &playbook.description,
            );
            validate_text(
                &mut errors,
                &format!("{path}.objective"),
                &playbook.objective,
            );
            validate_sections(
                &mut errors,
                &format!("{path}.sections"),
                &playbook.sections,
                false,
            );

            let mut combined_ids = HashSet::new();
            let mut combined_titles = HashSet::new();
            for section in self
                .output_contract
                .sections
                .iter()
                .chain(playbook.sections.iter())
            {
                let normalized_title = section.title.trim().to_lowercase();
                if !combined_ids.insert(section.id.as_str()) {
                    push_error(
                        &mut errors,
                        ValidationErrorCode::DuplicateValue,
                        format!("{path}.sections"),
                        format!(
                            "section ID '{}' is duplicated in the rendered output",
                            section.id
                        ),
                    );
                }
                if !combined_titles.insert(normalized_title) {
                    push_error(
                        &mut errors,
                        ValidationErrorCode::DuplicateValue,
                        format!("{path}.sections"),
                        format!(
                            "section title '{}' is duplicated in the rendered output",
                            section.title
                        ),
                    );
                }
            }
        }

        finish(errors)
    }
}

impl Validate for EvalPlan {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = Vec::new();
        validate_eval_plan_base(self, &mut errors);
        finish(errors)
    }
}

impl EvalPlan {
    pub fn validate_for_profile(
        &self,
        profile: &ExpertProfileVersion,
    ) -> Result<(), ValidationErrors> {
        let mut errors = Vec::new();
        validate_eval_plan_base(self, &mut errors);

        let playbook_ids: HashSet<Uuid> = profile.playbooks.iter().map(|item| item.id).collect();
        let covered_ids: HashSet<Uuid> = self.cases.iter().map(|item| item.playbook_id).collect();

        for (index, case) in self.cases.iter().enumerate() {
            if !playbook_ids.contains(&case.playbook_id) {
                push_error(
                    &mut errors,
                    ValidationErrorCode::InvalidReference,
                    format!("$.cases[{index}].playbook_id"),
                    "evaluation case references a playbook that is not embedded in the profile",
                );
                continue;
            }

            let playbook = profile
                .playbooks
                .iter()
                .find(|playbook| playbook.id == case.playbook_id)
                .expect("playbook membership was checked above");
            let section_ids: HashSet<&str> = profile
                .output_contract
                .sections
                .iter()
                .chain(playbook.sections.iter())
                .map(|section| section.id.as_str())
                .collect();
            for (assertion_index, assertion) in case.assertions.hard.iter().enumerate() {
                if let HardAssertion::SectionPresent { section_id } = assertion {
                    if !section_ids.contains(section_id.as_str()) {
                        push_error(
                            &mut errors,
                            ValidationErrorCode::InvalidReference,
                            format!(
                                "$.cases[{index}].assertions.hard[{assertion_index}].section_id"
                            ),
                            "section assertion references a section that is not rendered by the selected playbook",
                        );
                    }
                }
            }
        }

        for (index, playbook) in profile.playbooks.iter().enumerate() {
            if !covered_ids.contains(&playbook.id) {
                push_error(
                    &mut errors,
                    ValidationErrorCode::EmptyEvalPlan,
                    "$.cases",
                    format!("embedded playbook at $.playbooks[{index}] has no evaluation case"),
                );
            }
        }

        finish(errors)
    }
}

pub fn parse_profile_json(input: &str) -> Result<ExpertProfileVersion, ValidationErrors> {
    check_json_input(input)?;
    let profile = serde_json::from_str::<ExpertProfileVersion>(input).map_err(schema_error)?;
    profile.validate()?;
    Ok(profile)
}

pub fn parse_eval_plan_json(input: &str) -> Result<EvalPlan, ValidationErrors> {
    check_json_input(input)?;
    let plan = serde_json::from_str::<EvalPlan>(input).map_err(schema_error)?;
    plan.validate()?;
    Ok(plan)
}

fn validate_eval_plan_base(plan: &EvalPlan, errors: &mut Vec<ValidationError>) {
    if plan.schema_version != EXPERT_PROFILE_SCHEMA_VERSION {
        push_error(
            errors,
            ValidationErrorCode::SchemaMismatch,
            "$.schema_version",
            format!(
                "expected schema version {EXPERT_PROFILE_SCHEMA_VERSION}, got {}",
                plan.schema_version
            ),
        );
    }

    if plan.fixtures.is_empty() {
        push_error(
            errors,
            ValidationErrorCode::EmptyEvalPlan,
            "$.fixtures",
            "at least one synthetic fixture is required",
        );
    }
    if plan.cases.is_empty() {
        push_error(
            errors,
            ValidationErrorCode::EmptyEvalPlan,
            "$.cases",
            "at least one evaluation case is required",
        );
    }
    if plan.cases.len() > MAX_CASES {
        push_limit_error(errors, "$.cases", MAX_CASES);
    }

    let mut fixtures = HashMap::new();
    for (index, fixture) in plan.fixtures.iter().enumerate() {
        let path = format!("$.fixtures[{index}]");
        validate_identifier(errors, &format!("{path}.id"), &fixture.id);
        validate_text(errors, &format!("{path}.source"), &fixture.source);
        if !fixture.source.starts_with("synthetic:") {
            push_error(
                errors,
                ValidationErrorCode::SchemaMismatch,
                format!("{path}.source"),
                "phase-1 evaluation fixtures must declare a synthetic: source",
            );
        }
        if fixture.transcript_text.trim().is_empty() {
            push_error(
                errors,
                ValidationErrorCode::EmptyEvalPlan,
                format!("{path}.transcript_text"),
                "fixture transcript cannot be empty",
            );
        }
        if fixture.transcript_text.len() > MAX_FIXTURE_BYTES {
            push_limit_error(
                errors,
                &format!("{path}.transcript_text"),
                MAX_FIXTURE_BYTES,
            );
        }
        if fixtures.insert(fixture.id.as_str(), fixture).is_some() {
            push_error(
                errors,
                ValidationErrorCode::DuplicateValue,
                format!("{path}.id"),
                "fixture IDs must be unique",
            );
        }

        let expected_hash = hash_fixture_text(&fixture.transcript_text);
        if fixture.content_hash != expected_hash {
            push_error(
                errors,
                ValidationErrorCode::DigestMismatch,
                format!("{path}.content_hash"),
                "fixture content hash does not match transcript_text",
            );
        }
    }

    let mut case_ids = HashSet::new();
    for (index, case) in plan.cases.iter().enumerate() {
        let path = format!("$.cases[{index}]");
        validate_identifier(errors, &format!("{path}.id"), &case.id);
        if !case_ids.insert(case.id.as_str()) {
            push_error(
                errors,
                ValidationErrorCode::DuplicateValue,
                format!("{path}.id"),
                "case IDs must be unique",
            );
        }
        if !fixtures.contains_key(case.fixture_id.as_str()) {
            push_error(
                errors,
                ValidationErrorCode::InvalidReference,
                format!("{path}.fixture_id"),
                "evaluation case references an unknown fixture",
            );
        }
        if case.assertions.hard.is_empty() {
            push_error(
                errors,
                ValidationErrorCode::EmptyEvalPlan,
                format!("{path}.assertions.hard"),
                "each evaluation case requires at least one hard assertion",
            );
        }

        for (assertion_index, assertion) in case.assertions.hard.iter().enumerate() {
            match assertion {
                HardAssertion::SchemaCompliance => {}
                HardAssertion::SectionPresent { section_id } => validate_identifier(
                    errors,
                    &format!("{path}.assertions.hard[{assertion_index}].section_id"),
                    section_id,
                ),
                HardAssertion::LiteralPresent { value }
                | HardAssertion::LiteralAbsent { value } => validate_text(
                    errors,
                    &format!("{path}.assertions.hard[{assertion_index}].value"),
                    value,
                ),
            }
        }

        for (assertion_index, assertion) in case.assertions.semantic.iter().enumerate() {
            match assertion {
                SemanticAssertion::Rubric {
                    question,
                    threshold,
                    ..
                } => {
                    validate_text(
                        errors,
                        &format!("{path}.assertions.semantic[{assertion_index}].question"),
                        question,
                    );
                    validate_score(
                        errors,
                        &format!("{path}.assertions.semantic[{assertion_index}].threshold"),
                        *threshold,
                    );
                }
            }
        }
    }

    if !(2..=10).contains(&plan.policy.activation_runs_per_case) {
        push_error(
            errors,
            ValidationErrorCode::LimitExceeded,
            "$.policy.activation_runs_per_case",
            "activation_runs_per_case must be between 2 and 10",
        );
    }
    if !plan.policy.all_hard_runs_must_pass {
        push_error(
            errors,
            ValidationErrorCode::SchemaMismatch,
            "$.policy.all_hard_runs_must_pass",
            "phase 1 requires every hard assertion to pass on every run",
        );
    }
    validate_score(
        errors,
        "$.policy.semantic_min_score",
        plan.policy.semantic_min_score,
    );
    if plan.policy.timeout_seconds == 0 || plan.policy.timeout_seconds > 3600 {
        push_error(
            errors,
            ValidationErrorCode::LimitExceeded,
            "$.policy.timeout_seconds",
            "timeout_seconds must be between 1 and 3600",
        );
    }
    if !(-1.0..=0.0).contains(&plan.regression_policy.semantic_delta_floor) {
        push_error(
            errors,
            ValidationErrorCode::LimitExceeded,
            "$.regression_policy.semantic_delta_floor",
            "semantic_delta_floor must be between -1.0 and 0.0",
        );
    }
}

fn check_json_input(input: &str) -> Result<(), ValidationErrors> {
    let mut errors = Vec::new();
    if input.len() > MAX_JSON_BYTES {
        push_limit_error(&mut errors, "$", MAX_JSON_BYTES);
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for character in input.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '{' | '[' => {
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    push_limit_error(&mut errors, "$", MAX_JSON_DEPTH);
                    break;
                }
            }
            '}' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    finish(errors)
}

fn validate_sections(
    errors: &mut Vec<ValidationError>,
    path: &str,
    sections: &[OutputSection],
    require_non_empty: bool,
) {
    if require_non_empty && sections.is_empty() {
        push_error(
            errors,
            ValidationErrorCode::SchemaMismatch,
            path,
            "at least one output section is required",
        );
    }
    if sections.len() > MAX_SECTIONS {
        push_limit_error(errors, path, MAX_SECTIONS);
    }

    let mut ids = HashSet::new();
    let mut titles = HashSet::new();
    for (index, section) in sections.iter().enumerate() {
        let section_path = format!("{path}[{index}]");
        validate_identifier(errors, &format!("{section_path}.id"), &section.id);
        validate_text(errors, &format!("{section_path}.title"), &section.title);
        validate_text(
            errors,
            &format!("{section_path}.instruction"),
            &section.instruction,
        );
        if !ids.insert(section.id.as_str()) {
            push_error(
                errors,
                ValidationErrorCode::DuplicateValue,
                format!("{section_path}.id"),
                "section IDs must be unique",
            );
        }
        if !titles.insert(section.title.trim().to_lowercase()) {
            push_error(
                errors,
                ValidationErrorCode::DuplicateValue,
                format!("{section_path}.title"),
                "section titles must be unique",
            );
        }
    }
}

fn validate_non_empty_text_list(
    errors: &mut Vec<ValidationError>,
    path: &str,
    values: &[String],
    max_items: usize,
) {
    if values.is_empty() {
        push_error(
            errors,
            ValidationErrorCode::SchemaMismatch,
            path,
            "at least one value is required",
        );
    }
    validate_text_list(errors, path, values, max_items);
}

fn validate_text_list(
    errors: &mut Vec<ValidationError>,
    path: &str,
    values: &[String],
    max_items: usize,
) {
    if values.len() > max_items {
        push_limit_error(errors, path, max_items);
    }
    for (index, value) in values.iter().enumerate() {
        validate_text(errors, &format!("{path}[{index}]"), value);
    }
}

fn validate_text(errors: &mut Vec<ValidationError>, path: &str, value: &str) {
    if value.trim().is_empty() {
        push_error(
            errors,
            ValidationErrorCode::SchemaMismatch,
            path,
            "value cannot be empty",
        );
    }
    if value.len() > MAX_STRING_BYTES {
        push_limit_error(errors, path, MAX_STRING_BYTES);
    }
}

fn validate_identifier(errors: &mut Vec<ValidationError>, path: &str, value: &str) {
    validate_text(errors, path, value);
    if !value.is_empty()
        && !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        push_error(
            errors,
            ValidationErrorCode::SchemaMismatch,
            path,
            "identifier may contain only ASCII letters, digits, '-' and '_'",
        );
    }
}

fn validate_language(errors: &mut Vec<ValidationError>, path: &str, value: &str) {
    if value.len() != 2
        || !value
            .chars()
            .all(|character| character.is_ascii_lowercase())
    {
        push_error(
            errors,
            ValidationErrorCode::SchemaMismatch,
            path,
            "language must be a lowercase ISO 639-1 code",
        );
    }
}

fn validate_score(errors: &mut Vec<ValidationError>, path: &str, score: f64) {
    if !score.is_finite() || !(0.0..=1.0).contains(&score) {
        push_error(
            errors,
            ValidationErrorCode::LimitExceeded,
            path,
            "score must be a finite number between 0.0 and 1.0",
        );
    }
}

fn schema_error(error: serde_json::Error) -> ValidationErrors {
    ValidationErrors(vec![ValidationError {
        code: if error.to_string().contains("unknown field") {
            ValidationErrorCode::UnknownField
        } else {
            ValidationErrorCode::SchemaMismatch
        },
        path: "$".to_string(),
        message: error.to_string(),
    }])
}

fn push_limit_error(errors: &mut Vec<ValidationError>, path: impl Into<String>, max: usize) {
    push_error(
        errors,
        ValidationErrorCode::LimitExceeded,
        path,
        format!("value exceeds the phase-1 limit of {max}"),
    );
}

fn push_error(
    errors: &mut Vec<ValidationError>,
    code: ValidationErrorCode,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    errors.push(ValidationError {
        code,
        path: path.into(),
        message: message.into(),
    });
}

fn finish(errors: Vec<ValidationError>) -> Result<(), ValidationErrors> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationErrors(errors))
    }
}
