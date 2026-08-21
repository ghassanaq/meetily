//! Governed evidence composition for broad professional-introduction questions.
//!
//! This follows the same boundary as Mishkat's write service: classify an explicit
//! request, build a deterministic brief, and leave the provider responsible only
//! for rendering that brief. Specific questions continue to use lexical retrieval.

use super::IdentityRecordCategory;

pub const PROFESSIONAL_INTRODUCTION_PROFILE: &str = "professional-introduction/v1";
pub const TOTAL_EVIDENCE_CHAR_BUDGET: usize = 7_000;
pub const PER_RECORD_CHAR_BUDGET: usize = 1_200;

const INTRODUCTION_PATTERNS: &[&str] = &[
    "tell me about yourself",
    "tell us about yourself",
    "introduce yourself",
    "walk me through your background",
    "walk us through your background",
    "overview of your background",
    "summarize your professional background",
    "describe your professional background",
    "what is your professional background",
];

const CAREER_MARKERS: &[&str] = &[
    "background",
    "career",
    "coordination",
    "experience",
    "leadership",
    "management",
    "operations",
    "programme",
    "program",
    "professional",
];

pub(super) fn is_professional_introduction(question: &str) -> bool {
    let normalized = normalize_phrase(question);
    INTRODUCTION_PATTERNS
        .iter()
        .any(|pattern| normalized.contains(pattern))
}

/// Lower values are selected first. CV evidence is the anchor; career evidence
/// and role evidence can then enrich the brief without random UUID tie-breaking.
pub(super) fn evidence_priority(
    category: IdentityRecordCategory,
    title: &str,
    tags: &[String],
) -> u8 {
    if category == IdentityRecordCategory::Cv {
        return 0;
    }
    let searchable = format!("{} {}", title.to_lowercase(), tags.join(" ").to_lowercase());
    if CAREER_MARKERS
        .iter()
        .any(|marker| searchable.contains(marker))
    {
        return 1;
    }
    if category == IdentityRecordCategory::TermsOfReference {
        return 2;
    }
    3
}

pub(super) fn evidence_excerpt(value: &str, maximum_chars: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || maximum_chars == 0 {
        return None;
    }
    if trimmed.chars().count() <= maximum_chars {
        return Some(trimmed.to_string());
    }

    let mut used_chars = 0usize;
    let mut last_boundary = None;
    for (index, character) in trimmed.char_indices() {
        if used_chars >= maximum_chars {
            break;
        }
        used_chars += 1;
        if matches!(character, '.' | '!' | '?' | '\n') {
            last_boundary = Some(index + character.len_utf8());
        }
    }
    last_boundary.map(|boundary| trimmed[..boundary].trim().to_string())
}

fn normalize_phrase(value: &str) -> String {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_professional_introduction_phrasing_without_stripping_the_pattern() {
        assert!(is_professional_introduction(
            "Panel: Tell us about yourself."
        ));
        assert!(is_professional_introduction(
            "Could you walk me through your background?"
        ));
        assert!(!is_professional_introduction(
            "Tell us about your approach to staff safety."
        ));
    }

    #[test]
    fn excerpts_end_at_a_sentence_boundary() {
        let excerpt = evidence_excerpt("First complete sentence. Second sentence is longer.", 30)
            .expect("the first sentence fits");
        assert_eq!(excerpt, "First complete sentence.");
    }

    #[test]
    fn oversized_unbroken_content_is_omitted_instead_of_cut_mid_claim() {
        assert_eq!(evidence_excerpt(&"x".repeat(100), 20), None);
    }
}
