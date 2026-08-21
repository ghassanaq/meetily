//! Pure, local evaluation of explicitly enrolled authority-scope constraints.
//!
//! This module intentionally performs closed lexical matching. A clean result
//! means only that no enrolled rule matched; it is never a factuality verdict.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{normalize_authority_alias, AuthorityActionFamily, AuthorityConstraint};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityCheckStatus {
    NotConfigured,
    CheckedNoMatch,
    Warning,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityPolicyWarningCode {
    AuthorityScopeExpansion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityPolicyWarning {
    pub code: AuthorityPolicyWarningCode,
    pub rule_id: String,
    pub rule_label: String,
    pub sentence: String,
    pub matched_action: String,
    pub matched_context: Option<String>,
    pub matched_excluded_object: String,
    pub excluded_start_utf16: u32,
    pub excluded_end_utf16: u32,
    pub evidence_record_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityCheckResult {
    pub status: AuthorityCheckStatus,
    pub evaluated_rule_count: u32,
    pub warnings: Vec<AuthorityPolicyWarning>,
}

#[derive(Debug)]
struct Token {
    normalized: String,
    start_byte: usize,
    end_byte: usize,
    start_utf16: usize,
    end_utf16: usize,
}

#[derive(Debug)]
struct PendingWarning {
    sentence_offset: usize,
    warning: AuthorityPolicyWarning,
}

pub(crate) fn evaluate_authority_scope(
    answer: &str,
    constraints: &[AuthorityConstraint],
) -> AuthorityCheckResult {
    if constraints.is_empty() {
        return AuthorityCheckResult {
            status: AuthorityCheckStatus::NotConfigured,
            evaluated_rule_count: 0,
            warnings: Vec::new(),
        };
    }

    let mut pending = Vec::new();
    for (sentence_offset, sentence) in sentences(answer) {
        let tokens = tokens(sentence);
        if tokens.is_empty() {
            continue;
        }
        for rule in constraints {
            for excluded in &rule.excluded_objects {
                let mut search_from = 0usize;
                while let Some(excluded_range) =
                    find_phrase(&tokens, excluded, search_from, tokens.len())
                {
                    search_from = excluded_range.0 + 1;
                    let (clause_start, clause_end) = clause_bounds(&tokens, excluded_range.0);
                    let matched_context = if rule.contexts.is_empty() {
                        Some(None)
                    } else {
                        rule.contexts.iter().find_map(|context| {
                            find_phrase(&tokens, context, clause_start, clause_end)
                                .map(|range| Some(original_phrase(sentence, &tokens, range)))
                        })
                    };
                    let Some(matched_context) = matched_context else {
                        continue;
                    };
                    let Some(action_range) = closest_action_before(
                        &tokens,
                        &rule.action_families,
                        clause_start,
                        excluded_range.0,
                    ) else {
                        continue;
                    };
                    if !has_first_person_subject(&tokens, clause_start, action_range.0)
                        || is_prospective(&tokens, clause_start, action_range.1)
                        || is_negated(
                            &tokens,
                            clause_start,
                            action_range,
                            excluded_range,
                            clause_end,
                        )
                    {
                        continue;
                    }

                    let excluded_start_utf16 = tokens[excluded_range.0].start_utf16;
                    let excluded_end_utf16 = tokens[excluded_range.1 - 1].end_utf16;
                    pending.push(PendingWarning {
                        sentence_offset,
                        warning: AuthorityPolicyWarning {
                            code: AuthorityPolicyWarningCode::AuthorityScopeExpansion,
                            rule_id: rule.id.clone(),
                            rule_label: rule.label.clone(),
                            sentence: sentence.to_string(),
                            matched_action: original_phrase(sentence, &tokens, action_range),
                            matched_context: matched_context.clone(),
                            matched_excluded_object: original_phrase(
                                sentence,
                                &tokens,
                                excluded_range,
                            ),
                            excluded_start_utf16: excluded_start_utf16
                                .try_into()
                                .unwrap_or(u32::MAX),
                            excluded_end_utf16: excluded_end_utf16.try_into().unwrap_or(u32::MAX),
                            evidence_record_ids: rule.evidence_record_ids.clone(),
                        },
                    });
                }
            }
        }
    }

    pending.sort_by(|left, right| {
        (
            left.sentence_offset,
            left.warning.excluded_start_utf16,
            left.warning.rule_id.as_str(),
        )
            .cmp(&(
                right.sentence_offset,
                right.warning.excluded_start_utf16,
                right.warning.rule_id.as_str(),
            ))
    });
    pending.dedup_by(|left, right| {
        left.sentence_offset == right.sentence_offset
            && left.warning.excluded_start_utf16 == right.warning.excluded_start_utf16
            && left.warning.excluded_end_utf16 == right.warning.excluded_end_utf16
    });
    let warnings = pending
        .into_iter()
        .map(|item| item.warning)
        .collect::<Vec<_>>();
    AuthorityCheckResult {
        status: if warnings.is_empty() {
            AuthorityCheckStatus::CheckedNoMatch
        } else {
            AuthorityCheckStatus::Warning
        },
        evaluated_rule_count: constraints.len().try_into().unwrap_or(u32::MAX),
        warnings,
    }
}

fn sentences(answer: &str) -> Vec<(usize, &str)> {
    let mut result = Vec::new();
    let mut start = 0usize;
    for (index, character) in answer.char_indices() {
        if !matches!(character, '.' | '!' | '?' | '\n') {
            continue;
        }
        let end = index + character.len_utf8();
        push_sentence(answer, start, end, &mut result);
        start = end;
    }
    push_sentence(answer, start, answer.len(), &mut result);
    result
}

fn push_sentence<'a>(
    answer: &'a str,
    start: usize,
    end: usize,
    result: &mut Vec<(usize, &'a str)>,
) {
    let raw = &answer[start..end];
    let trimmed_start = raw.trim_start();
    let leading = raw.len() - trimmed_start.len();
    let sentence = trimmed_start.trim_end();
    if !sentence.is_empty() {
        result.push((start + leading, sentence));
    }
}

fn tokens(sentence: &str) -> Vec<Token> {
    let mut result = Vec::new();
    let mut start = None;
    for (index, character) in sentence
        .char_indices()
        .chain(std::iter::once((sentence.len(), ' ')))
    {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
            continue;
        }
        let Some(start_byte) = start.take() else {
            continue;
        };
        let end_byte = index;
        result.push(Token {
            normalized: sentence[start_byte..end_byte].to_lowercase(),
            start_byte,
            end_byte,
            start_utf16: sentence[..start_byte].encode_utf16().count(),
            end_utf16: sentence[..end_byte].encode_utf16().count(),
        });
    }
    result
}

fn find_phrase(tokens: &[Token], phrase: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let normalized = normalize_authority_alias(phrase);
    let parts = normalized.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() || end.saturating_sub(start) < parts.len() {
        return None;
    }
    (start..=end - parts.len()).find_map(|index| {
        tokens[index..index + parts.len()]
            .iter()
            .zip(&parts)
            .all(|(token, part)| token.normalized == *part)
            .then_some((index, index + parts.len()))
    })
}

fn original_phrase(sentence: &str, tokens: &[Token], range: (usize, usize)) -> String {
    sentence[tokens[range.0].start_byte..tokens[range.1 - 1].end_byte].to_string()
}

fn clause_bounds(tokens: &[Token], at: usize) -> (usize, usize) {
    let start = tokens[..at]
        .iter()
        .rposition(|token| is_clause_boundary(&token.normalized))
        .map_or(0, |index| index + 1);
    let end = tokens[at..]
        .iter()
        .position(|token| is_clause_boundary(&token.normalized))
        .map_or(tokens.len(), |index| at + index);
    (start, end)
}

fn is_clause_boundary(token: &str) -> bool {
    matches!(
        token,
        "but" | "however" | "yet" | "although" | "whereas" | "while"
    )
}

fn closest_action_before(
    tokens: &[Token],
    families: &[AuthorityActionFamily],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let mut closest: Option<(usize, usize)> = None;
    for family in families {
        for form in action_forms(*family) {
            let phrase = form.join(" ");
            let mut search_from = start;
            while let Some(range) = find_phrase(tokens, &phrase, search_from, end) {
                closest = match closest {
                    Some(current) if current.0 > range.0 => Some(current),
                    _ => Some(range),
                };
                search_from = range.0 + 1;
            }
        }
    }
    closest
}

fn action_forms(family: AuthorityActionFamily) -> &'static [&'static [&'static str]] {
    match family {
        AuthorityActionFamily::Manage => &[&["manage"], &["managed"], &["managing"]],
        AuthorityActionFamily::Lead => &[&["lead"], &["led"], &["leading"]],
        AuthorityActionFamily::Own => &[&["own"], &["owned"], &["owning"]],
        AuthorityActionFamily::Oversee => &[&["oversee"], &["oversaw"], &["overseeing"]],
        AuthorityActionFamily::ResponsibleFor => &[&["responsible", "for"]],
        AuthorityActionFamily::Approve => &[&["approve"], &["approved"], &["approving"]],
        AuthorityActionFamily::Decide => &[&["decide"], &["decided"], &["deciding"]],
    }
}

fn has_first_person_subject(tokens: &[Token], start: usize, action_start: usize) -> bool {
    tokens[start..action_start]
        .iter()
        .any(|token| matches!(token.normalized.as_str(), "i" | "we" | "my" | "our"))
}

fn is_prospective(tokens: &[Token], start: usize, action_end: usize) -> bool {
    tokens[start..action_end].iter().any(|token| {
        matches!(
            token.normalized.as_str(),
            "would"
                | "will"
                | "could"
                | "should"
                | "might"
                | "may"
                | "if"
                | "intend"
                | "intended"
                | "plan"
                | "planned"
        )
    })
}

fn is_negated(
    tokens: &[Token],
    clause_start: usize,
    action: (usize, usize),
    excluded: (usize, usize),
    clause_end: usize,
) -> bool {
    let search_start = action.0.saturating_sub(3).max(clause_start);
    let search_end = excluded.1.min(clause_end);
    tokens[search_start..search_end].iter().any(|token| {
        matches!(
            token.normalized.as_str(),
            "not" | "never" | "no" | "without"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(
        id: &str,
        contexts: &[&str],
        families: &[AuthorityActionFamily],
        excluded: &[&str],
    ) -> AuthorityConstraint {
        AuthorityConstraint {
            id: id.to_string(),
            label: "Synthetic workstream boundary".to_string(),
            contexts: contexts.iter().map(|value| (*value).to_string()).collect(),
            action_families: families.to_vec(),
            permitted_objects: vec!["processing workstream".to_string()],
            excluded_objects: excluded.iter().map(|value| (*value).to_string()).collect(),
            evidence_record_ids: vec![Uuid::from_u128(1)],
        }
    }

    #[test]
    fn no_rules_is_not_configured_and_supported_objects_do_not_warn() {
        assert_eq!(
            evaluate_authority_scope("I managed the assigned team.", &[]).status,
            AuthorityCheckStatus::NotConfigured
        );
        let result = evaluate_authority_scope(
            "I managed the assigned team and led the processing workstream.",
            &[rule(
                "operation-boundary",
                &[],
                &[AuthorityActionFamily::Manage, AuthorityActionFamily::Lead],
                &["whole operation"],
            )],
        );
        assert_eq!(result.status, AuthorityCheckStatus::CheckedNoMatch);
    }

    #[test]
    fn excluded_operation_and_decision_objects_warn() {
        let result = evaluate_authority_scope(
            "I managed the whole operation. I approved clinical decisions.",
            &[
                rule(
                    "operation-boundary",
                    &[],
                    &[AuthorityActionFamily::Manage],
                    &["whole operation"],
                ),
                rule(
                    "decision-boundary",
                    &[],
                    &[AuthorityActionFamily::Approve],
                    &["clinical decisions"],
                ),
            ],
        );
        assert_eq!(result.status, AuthorityCheckStatus::Warning);
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn sole_ownership_warns_when_enrolled() {
        let result = evaluate_authority_scope(
            "I owned sole accountability for the programme budget.",
            &[rule(
                "budget-boundary",
                &[],
                &[AuthorityActionFamily::Own],
                &["sole accountability"],
            )],
        );
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn prospective_hypothetical_and_negated_claims_do_not_warn() {
        let constraint = rule(
            "operation-boundary",
            &[],
            &[AuthorityActionFamily::Manage],
            &["whole operation"],
        );
        for answer in [
            "I would manage the whole operation carefully.",
            "If appointed, I would manage the whole operation.",
            "I did not manage the whole operation.",
            "I managed the workstream, not the whole operation.",
        ] {
            assert!(
                evaluate_authority_scope(answer, std::slice::from_ref(&constraint))
                    .warnings
                    .is_empty()
            );
        }
    }

    #[test]
    fn negation_in_a_different_clause_does_not_hide_an_affirmative_claim() {
        let result = evaluate_authority_scope(
            "I did not manage the team, but I managed the whole operation.",
            &[rule(
                "operation-boundary",
                &[],
                &[AuthorityActionFamily::Manage],
                &["whole operation"],
            )],
        );
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn context_rules_are_narrow_while_context_free_rules_catch_vague_claims() {
        let contextual = rule(
            "named-operation",
            &["named event"],
            &[AuthorityActionFamily::Manage],
            &["whole operation"],
        );
        assert!(evaluate_authority_scope(
            "I managed the whole operation.",
            std::slice::from_ref(&contextual),
        )
        .warnings
        .is_empty());
        let context_free = rule(
            "general-operation",
            &[],
            &[AuthorityActionFamily::Manage],
            &["whole operation"],
        );
        assert_eq!(
            evaluate_authority_scope("I managed the whole operation.", &[context_free])
                .warnings
                .len(),
            1
        );

        assert!(evaluate_authority_scope(
            "I supported the named event, but I managed the whole operation.",
            &[contextual],
        )
        .warnings
        .is_empty());
    }

    #[test]
    fn compound_context_claim_highlights_only_the_excluded_object() {
        let answer =
            "I managed the processing workstream in Alpha and the whole operation in Beta.";
        let result = evaluate_authority_scope(
            answer,
            &[rule(
                "beta-boundary",
                &["Beta"],
                &[AuthorityActionFamily::Manage],
                &["whole operation"],
            )],
        );
        let warning = result.warnings.first().unwrap();
        assert_eq!(warning.matched_context.as_deref(), Some("Beta"));
        assert_eq!(warning.matched_excluded_object, "whole operation");
        let encoded = answer.encode_utf16().collect::<Vec<_>>();
        let highlighted = String::from_utf16(
            &encoded[warning.excluded_start_utf16 as usize..warning.excluded_end_utf16 as usize],
        )
        .unwrap();
        assert_eq!(highlighted, "whole operation");
    }

    #[test]
    fn unenrolled_paraphrases_remain_no_match() {
        let result = evaluate_authority_scope(
            "I ran the entire mission.",
            &[rule(
                "operation-boundary",
                &[],
                &[AuthorityActionFamily::Manage],
                &["whole operation"],
            )],
        );
        assert_eq!(result.status, AuthorityCheckStatus::CheckedNoMatch);
    }

    #[test]
    fn utf16_offsets_survive_non_ascii_prefixes() {
        let answer = "I coordinated café work, and I managed the whole operation.";
        let result = evaluate_authority_scope(
            answer,
            &[rule(
                "operation-boundary",
                &[],
                &[AuthorityActionFamily::Manage],
                &["whole operation"],
            )],
        );
        let warning = result.warnings.first().unwrap();
        let encoded = answer.encode_utf16().collect::<Vec<_>>();
        assert_eq!(
            String::from_utf16(
                &encoded
                    [warning.excluded_start_utf16 as usize..warning.excluded_end_utf16 as usize]
            )
            .unwrap(),
            "whole operation"
        );
    }

    #[test]
    fn snapshot_contract_serializes_authority_diagnostics_as_camel_case() {
        let result = evaluate_authority_scope(
            "I managed the whole operation.",
            &[rule(
                "operation-boundary",
                &[],
                &[AuthorityActionFamily::Manage],
                &["whole operation"],
            )],
        );
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["evaluatedRuleCount"], 1);
        assert!(json.get("evaluated_rule_count").is_none());
        let warning = &json["warnings"][0];
        assert_eq!(warning["ruleId"], "operation-boundary");
        assert_eq!(warning["matchedExcludedObject"], "whole operation");
        assert!(warning.get("rule_id").is_none());
    }

    #[test]
    fn repeated_aliases_and_overlapping_rules_are_deduplicated_deterministically() {
        let first = rule(
            "a-boundary",
            &[],
            &[AuthorityActionFamily::Manage],
            &["whole operation", "whole operation"],
        );
        let second = rule(
            "b-boundary",
            &[],
            &[AuthorityActionFamily::Manage],
            &["whole operation"],
        );
        let result = evaluate_authority_scope("I managed the whole operation.", &[second, first]);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0].rule_id, "a-boundary");
    }
}
