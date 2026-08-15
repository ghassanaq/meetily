use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;
use thiserror::Error;

use super::models::{
    EffectiveCapabilityRevision, EvalPlan, ExpertProfileVersion, ModelGenerationBinding,
};

const PROFILE_HASH_DOMAIN: &[u8] = b"meetily-profile-v1\0";
const EVAL_PLAN_HASH_DOMAIN: &[u8] = b"meetily-eval-plan-v1\0";
const FIXTURE_HASH_DOMAIN: &[u8] = b"meetily-eval-fixture-v1\0";
const MODEL_BINDING_HASH_DOMAIN: &[u8] = b"meetily-model-binding-v1\0";
const CAPABILITY_REVISION_HASH_DOMAIN: &[u8] = b"meetily-capability-revision-v1\0";
const PROMPT_RENDERER_HASH_DOMAIN: &[u8] = b"meetily-profile-prompt-renderer-v1\0";
const MAX_JCS_SAFE_INTEGER: u64 = 9_007_199_254_740_992;

#[derive(Debug, Error)]
pub enum HashError {
    #[error("failed to serialize content for canonical hashing: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("integer at {path} exceeds the RFC 8785 safe range")]
    UnsafeInteger { path: String },
}

pub fn hash_profile_version(profile: &ExpertProfileVersion) -> Result<String, HashError> {
    hash_serializable(PROFILE_HASH_DOMAIN, profile)
}

pub fn hash_eval_plan(plan: &EvalPlan) -> Result<String, HashError> {
    hash_serializable(EVAL_PLAN_HASH_DOMAIN, plan)
}

pub fn hash_fixture_text(transcript_text: &str) -> String {
    hash_bytes(FIXTURE_HASH_DOMAIN, transcript_text.as_bytes())
}

pub fn hash_model_binding(binding: &ModelGenerationBinding) -> Result<String, HashError> {
    hash_serializable(MODEL_BINDING_HASH_DOMAIN, binding)
}

pub fn hash_capability_revision(
    revision: &EffectiveCapabilityRevision,
) -> Result<String, HashError> {
    hash_serializable(CAPABILITY_REVISION_HASH_DOMAIN, revision)
}

pub fn prompt_renderer_hash() -> String {
    hash_bytes(
        PROMPT_RENDERER_HASH_DOMAIN,
        b"summary::processor::generate_meeting_summary/profile-renderer-v1/output-parser-v1",
    )
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, HashError> {
    let json_value = serde_json::to_value(value)?;
    ensure_jcs_safe_integers(&json_value, "$".to_string())?;
    Ok(serde_json_canonicalizer::to_vec(&json_value)?)
}

pub fn hash_serializable<T: Serialize>(domain: &[u8], value: &T) -> Result<String, HashError> {
    let canonical = canonical_json(value)?;
    Ok(hash_bytes(domain, &canonical))
}

fn ensure_jcs_safe_integers(value: &Value, path: String) -> Result<(), HashError> {
    match value {
        Value::Array(values) => {
            for (index, item) in values.iter().enumerate() {
                ensure_jcs_safe_integers(item, format!("{path}[{index}]"))?;
            }
        }
        Value::Object(values) => {
            for (key, item) in values {
                ensure_jcs_safe_integers(item, format!("{path}.{key}"))?;
            }
        }
        Value::Number(number) => {
            let unsafe_integer = number
                .as_i64()
                .map(|value| value.unsigned_abs() > MAX_JCS_SAFE_INTEGER)
                .or_else(|| number.as_u64().map(|value| value > MAX_JCS_SAFE_INTEGER))
                .unwrap_or(false);

            if unsafe_integer {
                return Err(HashError::UnsafeInteger { path });
            }
        }
        _ => {}
    }

    Ok(())
}

fn hash_bytes(domain: &[u8], value: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(value);
    let digest = digest.finalize();

    let mut output = String::with_capacity(7 + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
