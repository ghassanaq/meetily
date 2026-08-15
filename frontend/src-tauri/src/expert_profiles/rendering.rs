use serde::Serialize;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::summary::templates::{Template, TemplateSection};

use super::hashing::canonical_json;
use super::models::{ExpertProfileVersion, MeetingPlaybook, OutputSection, SectionFormat};
use super::validation::{Validate, ValidationErrors};

pub const PROMPT_RENDERER_VERSION: u32 = 1;
pub const OUTPUT_PARSER_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum ProfileRenderError {
    #[error(transparent)]
    Validation(#[from] ValidationErrors),
    #[error("playbook {0} is not embedded in this profile version")]
    PlaybookNotFound(Uuid),
    #[error("failed to render canonical profile configuration: {0}")]
    Canonicalization(#[from] super::hashing::HashError),
    #[error("profile configuration is not UTF-8")]
    InvalidCanonicalUtf8,
}

#[derive(Debug, Clone)]
pub struct ProfileRenderSpec {
    pub template_id: String,
    pub template: Template,
    pub configuration_context: String,
    pub playbook_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedProfileMarkdown {
    pub title: Option<String>,
    pub sections: HashMap<String, String>,
    pub section_order: Vec<String>,
    pub errors: Vec<String>,
}

impl ParsedProfileMarkdown {
    pub fn is_schema_compliant(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Serialize)]
struct PromptConfiguration<'a> {
    renderer_version: u32,
    identity: &'a super::models::ProfileIdentity,
    objectives: &'a [String],
    perspective: &'a str,
    style: &'a super::models::ProfileStyle,
    boundaries: &'a super::models::ProfileBoundaries,
    retrieval_policy: &'a super::models::RetrievalPolicy,
    selected_playbook: &'a MeetingPlaybook,
}

pub fn build_profile_render_spec(
    profile: &ExpertProfileVersion,
    playbook_id: Uuid,
) -> Result<ProfileRenderSpec, ProfileRenderError> {
    profile.validate()?;
    let playbook = profile
        .playbooks
        .iter()
        .find(|playbook| playbook.id == playbook_id)
        .ok_or(ProfileRenderError::PlaybookNotFound(playbook_id))?;

    let sections = profile
        .output_contract
        .sections
        .iter()
        .chain(playbook.sections.iter())
        .map(to_template_section)
        .collect();
    let template = Template {
        name: format!("{} — {}", profile.identity.name, playbook.name),
        description: playbook.description.clone(),
        sections,
    };

    let configuration = PromptConfiguration {
        renderer_version: PROMPT_RENDERER_VERSION,
        identity: &profile.identity,
        objectives: &profile.objectives,
        perspective: &profile.perspective,
        style: &profile.style,
        boundaries: &profile.boundaries,
        retrieval_policy: &profile.retrieval_policy,
        selected_playbook: playbook,
    };
    let canonical = canonical_json(&configuration)?;
    let canonical =
        String::from_utf8(canonical).map_err(|_| ProfileRenderError::InvalidCanonicalUtf8)?;
    // Free-form profile strings are data inside the outer user-context block.
    // Keep delimiter-shaped text from terminating that block in the raw prompt.
    let canonical = canonical.replace('<', "\\u003c").replace('>', "\\u003e");
    let configuration_context = format!(
        "<expert_profile_configuration renderer_version=\"{PROMPT_RENDERER_VERSION}\">\n{canonical}\n</expert_profile_configuration>"
    );

    Ok(ProfileRenderSpec {
        template_id: format!("expert-profile:{playbook_id}"),
        template,
        configuration_context,
        playbook_id,
    })
}

pub fn parse_profile_markdown(
    profile: &ExpertProfileVersion,
    playbook_id: Uuid,
    markdown: &str,
) -> Result<ParsedProfileMarkdown, ProfileRenderError> {
    let spec = build_profile_render_spec(profile, playbook_id)?;
    let expected_sections: Vec<(&str, &str, bool, SectionFormat)> = profile
        .output_contract
        .sections
        .iter()
        .chain(
            profile
                .playbooks
                .iter()
                .find(|item| item.id == playbook_id)
                .expect("render spec already verified playbook")
                .sections
                .iter(),
        )
        .map(|section| {
            (
                section.id.as_str(),
                section.title.as_str(),
                section.required,
                section.format,
            )
        })
        .collect();

    let mut title = None;
    let mut found = Vec::<(String, String)>::new();
    let mut current: Option<(String, String)> = None;
    let mut errors = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("# ") {
            if title.replace(value.trim().to_string()).is_some() {
                errors.push("the output contains more than one level-one title".to_string());
            }
            continue;
        }

        let heading = expected_sections
            .iter()
            .find_map(|(id, section_title, _, _)| {
                let bold = format!("**{section_title}**");
                let h2 = format!("## {section_title}");
                (trimmed == bold || trimmed == h2).then(|| (*id).to_string())
            });
        if let Some(section_id) = heading {
            if let Some(previous) = current.take() {
                found.push(previous);
            }
            current = Some((section_id, String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line);
        }
    }
    if let Some(previous) = current {
        found.push(previous);
    }

    let mut sections = HashMap::new();
    let mut section_order = Vec::new();
    for (id, body) in found {
        if sections
            .insert(id.clone(), body.trim().to_string())
            .is_some()
        {
            errors.push(format!("section '{id}' appears more than once"));
        }
        section_order.push(id);
    }

    if profile.output_contract.title_required
        && title.as_ref().is_none_or(|value| value.trim().is_empty())
    {
        errors.push("the required level-one title is missing or empty".to_string());
    }

    let expected_order: Vec<String> = expected_sections
        .iter()
        .map(|(id, _, _, _)| (*id).to_string())
        .collect();
    if section_order != expected_order {
        errors.push(format!(
            "sections are missing, duplicated, or out of order; expected {}",
            expected_order.join(", ")
        ));
    }

    for (id, _, required, format) in expected_sections {
        if required && sections.get(id).is_none_or(|body| body.trim().is_empty()) {
            errors.push(format!("required section '{id}' is missing or empty"));
            continue;
        }
        if let Some(body) = sections.get(id) {
            let non_empty_lines: Vec<&str> = body
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            match format {
                SectionFormat::List
                    if !non_empty_lines
                        .iter()
                        .any(|line| is_markdown_list_item(line)) =>
                {
                    errors.push(format!("section '{id}' must contain a Markdown list"));
                }
                SectionFormat::String if non_empty_lines.len() != 1 => {
                    errors.push(format!("section '{id}' must contain one non-empty line"));
                }
                _ => {}
            }
        }
    }

    spec.template
        .validate()
        .expect("validated profile always produces a valid summary template");

    Ok(ParsedProfileMarkdown {
        title,
        sections,
        section_order,
        errors,
    })
}

fn is_markdown_list_item(line: &str) -> bool {
    line.starts_with("- ")
        || line.starts_with("* ")
        || line.split_once(". ").is_some_and(|(prefix, _)| {
            !prefix.is_empty() && prefix.chars().all(|character| character.is_ascii_digit())
        })
}

fn to_template_section(section: &OutputSection) -> TemplateSection {
    TemplateSection {
        title: section.title.clone(),
        instruction: section.instruction.clone(),
        format: match section.format {
            SectionFormat::Paragraph => "paragraph",
            SectionFormat::List => "list",
            SectionFormat::String => "string",
        }
        .to_string(),
        item_format: None,
        example_item_format: None,
    }
}
