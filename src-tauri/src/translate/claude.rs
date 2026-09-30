use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{BatchRequest, Translator};
use crate::auth::CredentialProvider;
use crate::error::{AppError, AppResult};

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 16_000;
const EFFORT: &str = "medium";
const MAX_HTTP_ATTEMPTS: u32 = 4;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

pub struct ClaudeTranslator {
    http: reqwest::Client,
    credentials: Arc<dyn CredentialProvider>,
    model: String,
}

impl ClaudeTranslator {
    pub fn new(credentials: Arc<dyn CredentialProvider>, model: impl Into<String>) -> AppResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| AppError::Api(e.to_string()))?;
        Ok(Self {
            http,
            credentials,
            model: model.into(),
        })
    }

    async fn send(&self, body: &Value) -> AppResult<String> {
        let mut attempt = 1;
        loop {
            let credential = self.credentials.credential().await?;
            let (auth_name, auth_value) = credential.header();
            let betas = [Some(FALLBACK_BETA), credential.beta()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(",");
            let response = self
                .http
                .post(API_URL)
                .header(auth_name, auth_value)
                .header("anthropic-version", API_VERSION)
                .header("anthropic-beta", betas)
                .json(body)
                .send()
                .await;

            let retry_after = match response {
                Ok(resp) if resp.status().is_success() => {
                    return resp.text().await.map_err(|e| AppError::Api(e.to_string()));
                }
                Ok(resp) if is_retryable(resp.status()) && attempt < MAX_HTTP_ATTEMPTS => {
                    retry_after(&resp).unwrap_or_else(|| backoff(attempt))
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(AppError::Api(describe_error(status, &text)));
                }
                Err(e) if (e.is_timeout() || e.is_connect()) && attempt < MAX_HTTP_ATTEMPTS => {
                    backoff(attempt)
                }
                Err(e) => return Err(AppError::Api(e.to_string())),
            };
            tokio::time::sleep(retry_after).await;
            attempt += 1;
        }
    }
}

#[async_trait]
impl Translator for ClaudeTranslator {
    async fn translate(&self, request: BatchRequest<'_>) -> AppResult<Vec<String>> {
        let body = request_body(&self.model, &request);
        let response = self.send(&body).await?;
        parse_response(&response, request.lines.len())
    }
}

fn system_prompt(source: &str, target: &str) -> String {
    format!(
        "You translate video subtitles from {source} into {target}.\n\
         Each item is one on-screen subtitle cue. Auto-generated captions often split one \
         sentence across several cues, so read the items as a continuous transcript and make \
         the sequence read naturally in {target}, while keeping each translation aligned with \
         the content of its own cue. Never merge, split, drop or reorder items: return exactly \
         one translation for every id. Preserve line breaks inside an item. Keep names, numbers \
         and units accurate. The `context` lines precede this batch and are for reference only; \
         do not translate them."
    )
}

fn output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "translations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "text": { "type": "string" }
                    },
                    "required": ["id", "text"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["translations"],
        "additionalProperties": false
    })
}

fn request_body(model: &str, request: &BatchRequest<'_>) -> Value {
    let items: Vec<Value> = request
        .lines
        .iter()
        .enumerate()
        .map(|(id, text)| json!({ "id": id, "text": text }))
        .collect();
    let input = json!({ "context": request.context, "items": items });
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "fallbacks": "default",
        "output_config": {
            "effort": EFFORT,
            "format": { "type": "json_schema", "schema": output_schema() }
        },
        "system": system_prompt(request.source_language, request.target_language),
        "messages": [{ "role": "user", "content": input.to_string() }]
    })
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    stop_details: Option<StopDetails>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text { text: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct StopDetails {
    category: Option<String>,
    explanation: Option<String>,
}

#[derive(Deserialize)]
struct TranslationOutput {
    translations: Vec<TranslatedItem>,
}

#[derive(Deserialize)]
struct TranslatedItem {
    id: usize,
    text: String,
}

fn parse_response(body: &str, expected: usize) -> AppResult<Vec<String>> {
    let response: ApiResponse = serde_json::from_str(body)?;
    match response.stop_reason.as_deref() {
        Some("refusal") => {
            let details = response.stop_details;
            return Err(AppError::Api(format!(
                "request declined ({}): {}",
                details.as_ref().and_then(|d| d.category.clone()).unwrap_or_default(),
                details.and_then(|d| d.explanation).unwrap_or_default()
            )));
        }
        Some("max_tokens") => {
            return Err(AppError::Translation("response truncated at max_tokens".into()))
        }
        _ => {}
    }
    let text: String = response
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text),
            ContentBlock::Other => None,
        })
        .collect();
    let output: TranslationOutput = serde_json::from_str(&text)
        .map_err(|e| AppError::Translation(format!("malformed output: {e}")))?;
    align_by_id(output.translations, expected)
}

fn align_by_id(items: Vec<TranslatedItem>, expected: usize) -> AppResult<Vec<String>> {
    let mut slots: Vec<Option<String>> = vec![None; expected];
    for item in items {
        match slots.get_mut(item.id) {
            Some(slot @ None) => *slot = Some(item.text),
            _ => {
                return Err(AppError::Translation(format!(
                    "unexpected or duplicate id {}",
                    item.id
                )))
            }
        }
    }
    slots
        .into_iter()
        .enumerate()
        .map(|(id, slot)| slot.ok_or_else(|| AppError::Translation(format!("missing id {id}"))))
        .collect()
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() || status.as_u16() == 529
}

fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get("retry-after")?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(2u64.pow(attempt))
}

fn describe_error(status: StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(String::from))
        .unwrap_or_else(|| body.chars().take(300).collect());
    format!("HTTP {status}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_body(text: &str, stop_reason: &str) -> String {
        json!({
            "content": [
                { "type": "thinking", "thinking": "", "signature": "sig" },
                { "type": "text", "text": text }
            ],
            "stop_reason": stop_reason,
            "stop_details": null
        })
        .to_string()
    }

    fn request<'a>(lines: &'a [String], context: &'a [String]) -> BatchRequest<'a> {
        BatchRequest {
            source_language: "Korean",
            target_language: "Traditional Chinese as used in Taiwan",
            context,
            lines,
        }
    }

    #[test]
    fn body_numbers_items_and_requests_structured_output() {
        let lines = vec!["안녕".to_string(), "하세요".to_string()];
        let context = vec!["이전".to_string()];
        let body = request_body(DEFAULT_MODEL, &request(&lines, &context));

        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert!(body.get("thinking").is_none());
        let input: Value = serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(input["items"][1], json!({ "id": 1, "text": "하세요" }));
        assert_eq!(input["context"], json!(["이전"]));
        assert!(body["system"].as_str().unwrap().contains("into Traditional Chinese as used in Taiwan"));
    }

    #[test]
    fn parses_translations_in_id_order_skipping_thinking_blocks() {
        let text = r#"{"translations":[{"id":1,"text":"B"},{"id":0,"text":"A"}]}"#;
        assert_eq!(parse_response(&api_body(text, "end_turn"), 2).unwrap(), ["A", "B"]);
    }

    #[test]
    fn missing_or_duplicate_ids_are_translation_errors() {
        let missing = r#"{"translations":[{"id":0,"text":"A"}]}"#;
        let duplicate = r#"{"translations":[{"id":0,"text":"A"},{"id":0,"text":"B"}]}"#;
        let out_of_range = r#"{"translations":[{"id":5,"text":"A"}]}"#;
        for text in [missing, duplicate, out_of_range] {
            assert!(matches!(
                parse_response(&api_body(text, "end_turn"), 2),
                Err(AppError::Translation(_))
            ));
        }
    }

    #[test]
    fn refusal_is_reported_as_api_error() {
        let body = json!({
            "content": [],
            "stop_reason": "refusal",
            "stop_details": { "type": "refusal", "category": "cyber", "explanation": "x" }
        })
        .to_string();
        let err = parse_response(&body, 1).unwrap_err();
        assert!(matches!(err, AppError::Api(ref m) if m.contains("cyber")));
    }

    #[test]
    fn truncation_is_retryable_translation_error() {
        assert!(matches!(
            parse_response(&api_body("{", "max_tokens"), 1),
            Err(AppError::Translation(_))
        ));
    }

    #[test]
    fn error_body_message_is_extracted() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"bad token"}}"#;
        assert_eq!(
            describe_error(StatusCode::UNAUTHORIZED, body),
            "HTTP 401 Unauthorized: bad token"
        );
    }
}
