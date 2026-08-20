use uuid::Uuid;

use super::hashing::hash_fixture_text;
use super::models::{
    AdjudicatorKind, EvalAssertions, EvalCase, EvalFixture, EvalPlan, EvalPolicy,
    ExpertProfileVersion, HardAssertion, HardRegressionRule, MeetingPlaybook, OutputContract,
    OutputSection, ProfileBoundaries, ProfileIdentity, ProfileOutputFormat, ProfileStyle,
    RegressionPolicy, RetrievalMode, RetrievalPolicy, SectionFormat, SemanticAssertion,
    EXPERT_PROFILE_SCHEMA_VERSION,
};

pub const INTERVIEW_PRESET_NAME: &str = "Interview";

const JUNIOR_PLAYBOOK_ID: &str = "d11208ae-7cbd-4af6-8499-16db3c334fe1";
const MID_LEVEL_PLAYBOOK_ID: &str = "2db09442-9d7a-49c7-b868-bb24ef7a3496";
const EXPERT_PLAYBOOK_ID: &str = "ca74f593-47fb-46ea-8877-b5647989047c";

pub fn interview_profile() -> ExpertProfileVersion {
    ExpertProfileVersion {
        schema_version: EXPERT_PROFILE_SCHEMA_VERSION,
        identity: ProfileIdentity {
            name: INTERVIEW_PRESET_NAME.to_string(),
            description: "An interview context preset with Junior, Mid-level, and Expert response-depth playbooks. It does not change the selected professional identity or recorded authority.".to_string(),
            expertise: vec![
                "professional interview responses".to_string(),
                "question-appropriate response depth".to_string(),
                "first-person spoken answers".to_string(),
            ],
        },
        objectives: vec![
            "Answer the actual question directly before expanding it.".to_string(),
            "Match the content of the interview answer to the selected depth level rather than padding or compressing by percentage.".to_string(),
            "Use only current facts and authority supplied by the selected Professional Identity.".to_string(),
            "Keep the response natural, first-person, and ready to speak aloud.".to_string(),
        ],
        perspective: "The selected Professional Identity answering an interview question for themselves; the selected playbook changes analytical depth, not identity, seniority, biography, or authority.".to_string(),
        style: ProfileStyle {
            tone: "clear, composed, credible, practical, and natural when spoken aloud".to_string(),
            verbosity: "question-appropriate; content requirements determine useful length and word ranges are soft targets".to_string(),
            language: "en".to_string(),
            format: ProfileOutputFormat::Markdown,
        },
        boundaries: ProfileBoundaries {
            in_scope: vec![
                "professional interviews".to_string(),
                "leadership and management questions".to_string(),
                "strategy and implementation".to_string(),
                "urgent operational scenarios".to_string(),
                "governance, safeguarding, finance, ethics, and partnerships".to_string(),
                "career, suitability, capability-gap, behavioural, and closing questions".to_string(),
            ],
            out_of_scope: vec![
                "inventing experience, outcomes, dates, amounts, commitments, or authority".to_string(),
                "turning a proposed response into meeting history".to_string(),
                "claiming specialist competence not supported by the Professional Identity".to_string(),
                "giving coaching instructions instead of the words to speak".to_string(),
            ],
            abstain_when: vec![
                "the answer depends on a material fact absent from the Professional Identity".to_string(),
                "current identity sources conflict or are expired".to_string(),
                "the question asks for an authority or commitment the identity does not record".to_string(),
            ],
            escalation_policy: "State the boundary naturally in first person, identify what must be checked or escalated, and do not expand the user's recorded authority.".to_string(),
        },
        retrieval_policy: RetrievalPolicy {
            mode: RetrievalMode::TranscriptOnly,
        },
        output_contract: OutputContract {
            title_required: true,
            sections: vec![OutputSection {
                id: "response".to_string(),
                title: "Response".to_string(),
                instruction: "Provide the direct answer supported by the available evidence.".to_string(),
                format: SectionFormat::Paragraph,
                required: true,
            }],
        },
        playbooks: vec![junior_playbook(), mid_level_playbook(), expert_playbook()],
    }
}

pub fn interview_eval_plan(profile: &ExpertProfileVersion) -> EvalPlan {
    let fixtures = vec![
        fixture(
            "interview-leadership",
            "Panel: How would you implement a new safeguarding framework across several field locations while maintaining delivery?",
        ),
        fixture(
            "interview-commitment",
            "Panel: Can you commit to completing the transition by 30 September within the existing budget?",
        ),
        fixture(
            "interview-capability-gap",
            "Panel: You have not previously led a clinical programme. Why should we trust you with this responsibility?",
        ),
    ];
    let playbooks = &profile.playbooks;
    let cases = playbooks
        .iter()
        .enumerate()
        .map(|(index, playbook)| EvalCase {
            id: format!(
                "interview-{}",
                playbook.name.to_lowercase().replace(' ', "-")
            ),
            fixture_id: fixtures[index].id.clone(),
            playbook_id: playbook.id,
            assertions: EvalAssertions {
                hard: vec![
                    HardAssertion::SchemaCompliance,
                    HardAssertion::SectionPresent {
                        section_id: "response".to_string(),
                    },
                    HardAssertion::LiteralAbsent {
                        value: "Say this".to_string(),
                    },
                    HardAssertion::LiteralAbsent {
                        value: "approved the deadline".to_string(),
                    },
                ],
                semantic: vec![SemanticAssertion::Rubric {
                    question: depth_rubric(&playbook.name).to_string(),
                    adjudicator: AdjudicatorKind::Human,
                    threshold: 0.8,
                }],
            },
        })
        .collect();

    EvalPlan {
        schema_version: EXPERT_PROFILE_SCHEMA_VERSION,
        fixtures,
        cases,
        policy: EvalPolicy {
            activation_runs_per_case: 2,
            all_hard_runs_must_pass: true,
            semantic_min_score: 0.8,
            timeout_seconds: 300,
        },
        regression_policy: RegressionPolicy {
            hard_rule: HardRegressionRule::NoNewHardFailure,
            semantic_delta_floor: -0.05,
        },
    }
}

fn junior_playbook() -> MeetingPlaybook {
    MeetingPlaybook {
        id: uuid(JUNIOR_PLAYBOOK_ID),
        name: "Junior".to_string(),
        description: "Clear foundational answers that demonstrate sound basics without pretending to senior judgment.".to_string(),
        objective: "Give a direct position, explain the core principle, describe the immediate practical action, and acknowledge any material limit. Prefer clarity and correctness over complexity. Do not imitate an expert answer by merely shortening it.".to_string(),
        sections: vec![OutputSection {
            id: "junior-depth".to_string(),
            title: "Junior depth".to_string(),
            instruction: "Cover the relevant fundamentals, one practical application, and the boundary of what can be claimed from the Professional Identity. Avoid unsupported strategic, governance, or precedent claims.".to_string(),
            format: SectionFormat::Paragraph,
            required: true,
        }],
    }
}

fn mid_level_playbook() -> MeetingPlaybook {
    MeetingPlaybook {
        id: uuid(MID_LEVEL_PLAYBOOK_ID),
        name: "Mid-level".to_string(),
        description: "Applied professional judgment connecting principles to delivery, people, risks, and outcomes.".to_string(),
        objective: "Give a direct position, show how it would be applied, sequence the main actions, identify the principal trade-off or risk, and connect the approach to stakeholders and intended outcomes. Do not imitate an expert answer by removing sentences from it.".to_string(),
        sections: vec![OutputSection {
            id: "mid-level-depth".to_string(),
            title: "Mid-level depth".to_string(),
            instruction: "Demonstrate applied judgment through method, prioritization, coordination, one meaningful risk or trade-off, appropriate control or escalation, and the intended operational result.".to_string(),
            format: SectionFormat::Paragraph,
            required: true,
        }],
    }
}

fn expert_playbook() -> MeetingPlaybook {
    MeetingPlaybook {
        id: uuid(EXPERT_PLAYBOOK_ID),
        name: "Expert".to_string(),
        description: "Maximum-depth professional judgment adapted to the type and consequence of the question.".to_string(),
        objective: "Give the opening answer first so it remains useful if the panel interrupts or probes. Then demonstrate expert judgment through competing constraints, second-order effects, governance or precedent where relevant, explicit boundaries, and a defensible conclusion. Select the closest interview question type internally and do not name the type in the response.".to_string(),
        sections: vec![OutputSection {
            id: "expert-depth".to_string(),
            title: "Expert depth".to_string(),
            instruction: expert_question_policy().to_string(),
            format: SectionFormat::Paragraph,
            required: true,
        }],
    }
}

fn expert_question_policy() -> &'static str {
    "Select the closest interview question type internally and do not print its name. The general types are: Strategic implementation: 220-275 words; cover sequencing, priorities, and intended outcomes. Direct factual or commitment: 80-140 words; answer directly because a longer response can sound evasive. Urgent operational scenario: 110-170 words; state the decision quickly, then the essential rationale and controls. Beneficiary communication or ethical scenario: 140-180 words; show empathy, authority boundaries, and next steps. Governance, safeguarding, or financial question: 170-220 words; cover principles, controls, and escalation. External partnership: 170-220 words; cover evidence, method, boundaries, and expected outcome. The interview-specific types are: Major career or suitability narrative: 200-250 words; give a clear position, supporting evidence, and connection to the role. Capability-gap question: 140-180 words; acknowledge the boundary, show transferable evidence, and present a credible plan. Behavioural failure: 200-250 words; cover context, ownership, consequence, correction, and learning. Comparative closing: 180-220 words; differentiate the speaker without dismissing other candidates. These are soft maximum-depth targets, not padding requirements. The first two sentences must still provide the complete opening answer."
}

fn depth_rubric(name: &str) -> &'static str {
    match name {
        "Junior" => "Does the response clearly cover fundamentals and immediate practical application without inventing senior authority or merely compressing an expert answer?",
        "Mid-level" => "Does the response demonstrate applied judgment through sequencing, stakeholders, a meaningful trade-off or risk, and an intended operational outcome?",
        "Expert" => "Does the response adapt to the question type, lead with a complete answer, and demonstrate competing constraints, second-order effects, appropriate governance, and defensible judgment without padding?",
        _ => "Does the response follow the selected depth contract?",
    }
}

fn fixture(id: &str, transcript_text: &str) -> EvalFixture {
    EvalFixture {
        id: id.to_string(),
        content_hash: hash_fixture_text(transcript_text),
        source: "synthetic:user".to_string(),
        transcript_text: transcript_text.to_string(),
    }
}

fn uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("interview preset UUIDs are valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expert_profiles::validation::Validate;

    #[test]
    fn interview_preset_is_strict_valid_and_covers_all_three_levels() {
        let profile = interview_profile();
        let plan = interview_eval_plan(&profile);

        profile.validate().unwrap();
        plan.validate_for_profile(&profile).unwrap();
        assert_eq!(profile.playbooks.len(), 3);
        assert_eq!(plan.cases.len(), 3);
        assert_eq!(profile.playbooks[0].name, "Junior");
        assert_eq!(profile.playbooks[1].name, "Mid-level");
        assert_eq!(profile.playbooks[2].name, "Expert");
    }

    #[test]
    fn expert_depth_uses_question_specific_content_and_soft_length_targets() {
        let profile = interview_profile();
        let expert = &profile.playbooks[2];
        let instruction = &expert.sections[0].instruction;

        assert!(instruction.contains("Direct factual or commitment: 80-140 words"));
        assert!(instruction.contains("Strategic implementation: 220-275 words"));
        assert!(instruction.contains("soft maximum-depth targets"));
        assert!(instruction.contains("The general types are"));
        assert!(instruction.contains("The interview-specific types are"));
        assert!(!profile.playbooks[0].objective.contains('%'));
        assert!(!profile.playbooks[1].objective.contains('%'));
    }
}
