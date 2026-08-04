//! "Explain + Fix": takes a detected architectural violation and asks Claude
//! to explain it in plain language and suggest a concrete fix. This is what
//! turns CKB from "here's a list of problems" into "here's what to actually
//! do about it" — the single biggest gap between a linter and something an
//! AI coding agent (or a human) can act on directly.
//!
//! Requires `ANTHROPIC_API_KEY` to be set. If it isn't, callers get a clear
//! error rather than a silent no-op or fabricated explanation.

use ckb_core::DriftViolation;
use serde::{Deserialize, Serialize};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
// Deliberately not hardcoding a specific dated model snapshot here, since
// those roll over — configurable via CKB_EXPLAIN_MODEL so operators can bump
// it without a code change. Defaults to a fast, cheap model since this is a
// short, well-scoped explanation task, not open-ended reasoning.
const DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainFixResponse {
    pub explanation: String,
    pub suggested_fix: String,
    pub model_used: String,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<AnthropicMessage>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Deserialize)]
struct AnthropicErrorBody {
    error: AnthropicErrorDetail,
}

#[derive(Deserialize)]
struct AnthropicErrorDetail {
    message: String,
}

fn build_prompt(violation: &DriftViolation) -> String {
    format!(
        "An architectural analysis tool detected this violation in a codebase:\n\n\
        - Kind: {:?}\n\
        - Severity: {:?}\n\
        - From: {}\n\
        - To: {}\n\
        - Boundary: {}\n\
        - Message: {}\n\n\
        Respond with EXACTLY two sections, in this format, and nothing else:\n\n\
        EXPLANATION:\n\
        <2-3 sentences explaining, for a developer who didn't write this code, why this specific violation matters architecturally — not a generic definition of the violation kind.>\n\n\
        SUGGESTED_FIX:\n\
        <A concrete, specific suggestion for how to resolve THIS violation — reference the actual symbols/files named above. If a suggested_fix was already provided below, refine or validate it rather than ignoring it. Keep it to 2-4 sentences; this is a suggestion to act on, not a full patch.>\n\n\
        {}",
        violation.kind,
        violation.severity,
        violation.from.0,
        violation.to.0,
        violation.boundary,
        violation.message,
        violation.suggested_fix.as_ref()
            .map(|f| format!("Existing suggested_fix from the analyzer to consider: {}", f))
            .unwrap_or_default(),
    )
}

fn parse_response(text: &str) -> (String, String) {
    let explanation_marker = "EXPLANATION:";
    let fix_marker = "SUGGESTED_FIX:";

    let explanation_start = text.find(explanation_marker).map(|i| i + explanation_marker.len());
    let fix_start = text.find(fix_marker);

    match (explanation_start, fix_start) {
        (Some(e_start), Some(f_start)) if f_start > e_start => {
            let explanation = text[e_start..f_start].trim().to_string();
            let fix = text[f_start + fix_marker.len()..].trim().to_string();
            (explanation, fix)
        }
        _ => {
            // Model didn't follow the format exactly — better to return the
            // raw text as the explanation than to silently drop it.
            (text.trim().to_string(), String::new())
        }
    }
}

/// Low-level Claude API call, shared by `explain_violation` and the Q&A
/// feature in `ask.rs`. Returns the concatenated text content of the
/// response.
pub async fn call_claude(system: &str, user_content: String, max_tokens: u32, api_key: &str) -> Result<(String, String), String> {
    let model = std::env::var("CKB_EXPLAIN_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    let request_body = AnthropicRequest {
        model: model.clone(),
        max_tokens,
        system: system.to_string(),
        messages: vec![AnthropicMessage {
            role: "user",
            content: user_content,
        }],
    };

    let client = reqwest::Client::new();
    let response = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to reach Anthropic API: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<AnthropicErrorBody>(&body_text)
            .map(|b| b.error.message)
            .unwrap_or(body_text);
        return Err(format!("Anthropic API returned {}: {}", status, message));
    }

    let parsed: AnthropicResponse = response.json().await
        .map_err(|e| format!("Failed to parse Anthropic API response: {}", e))?;

    let full_text: String = parsed.content.iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text.clone())
        .collect::<Vec<_>>()
        .join("\n");

    Ok((full_text, model))
}

pub async fn explain_violation(violation: &DriftViolation, api_key: &str) -> Result<ExplainFixResponse, String> {
    let (full_text, model) = call_claude(
        "You are an expert software architect reviewing a single detected architectural violation. Be specific and concise. Do not restate the input verbatim.",
        build_prompt(violation),
        512,
        api_key,
    ).await?;

    let (explanation, suggested_fix) = parse_response(&full_text);

    Ok(ExplainFixResponse {
        explanation,
        suggested_fix,
        model_used: model,
    })
}
