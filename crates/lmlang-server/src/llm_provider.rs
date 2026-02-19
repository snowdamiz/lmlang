//! Shared OpenAI-compatible provider chat client.

use serde::Deserialize;
use serde_json::json;

use crate::concurrency::AgentLlmConfig;
use crate::error::ApiError;

pub async fn run_external_chat(
    llm: &AgentLlmConfig,
    user_message: &str,
) -> Result<String, ApiError> {
    run_external_chat_internal(llm, user_message, false).await
}

/// Runs chat with JSON-mode preference enabled.
///
/// The provider is asked to return a JSON object payload in assistant content.
pub async fn run_external_chat_json(
    llm: &AgentLlmConfig,
    user_message: &str,
) -> Result<String, ApiError> {
    run_external_chat_internal(llm, user_message, true).await
}

async fn run_external_chat_internal(
    llm: &AgentLlmConfig,
    user_message: &str,
    json_mode: bool,
) -> Result<String, ApiError> {
    let provider = llm.provider.as_deref().unwrap_or_default();
    let base_url = match provider {
        "openrouter" => llm
            .api_base_url
            .clone()
            .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string()),
        "openai_compatible" => llm.api_base_url.clone().ok_or_else(|| {
            ApiError::BadRequest(
                "openai_compatible provider requires api_base_url in agent config".to_string(),
            )
        })?,
        other => {
            return Err(ApiError::BadRequest(format!(
                "unsupported provider '{}': use openrouter or openai_compatible",
                other
            )))
        }
    };

    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let api_key = llm.api_key.clone().unwrap_or_default();
    let model = llm.model.clone().unwrap_or_default();

    let mut messages = Vec::new();
    if let Some(system_prompt) = llm.system_prompt.clone() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": system_prompt
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": user_message
    }));

    let client = reqwest::Client::new();
    let mut req = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");

    let mut body = json!({
            "model": model,
            "messages": messages
    });

    if json_mode {
        body["response_format"] = json!({ "type": "json_object" });
    }

    req = req.json(&body);

    if provider == "openrouter" {
        req = req
            .header("HTTP-Referer", "https://localhost:3000")
            .header("X-Title", "lmlang dashboard");
    }

    let response = req
        .send()
        .await
        .map_err(|err| ApiError::InternalError(format!("provider request failed: {}", err)))?;

    let status = response.status();
    let body_text = response.text().await.map_err(|err| {
        ApiError::InternalError(format!("provider response read failed: {}", err))
    })?;

    if !status.is_success() {
        return Err(ApiError::BadRequest(format!(
            "provider request failed ({}): {}",
            status, body_text
        )));
    }

    let parsed: OpenAiCompatibleChatResponse = serde_json::from_str(&body_text).map_err(|err| {
        ApiError::InternalError(format!("provider response parse failed: {}", err))
    })?;

    let content = parsed
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::InternalError("provider response missing assistant content".to_string())
        })?;

    Ok(content)
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleChatResponse {
    choices: Vec<OpenAiCompatibleChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleChoice {
    message: OpenAiCompatibleMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleMessage {
    content: Option<String>,
}
