use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::SearchError;

#[derive(Clone)]
pub struct HermesClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

#[derive(Debug, Clone, Copy)]
pub struct HermesCapabilities {
    pub run_submission: bool,
    pub run_events_sse: bool,
    pub run_stop: bool,
}

#[derive(Debug, Serialize)]
struct StartRunRequest<'a> {
    input: &'a str,
    instructions: &'a str,
    session_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct StartRunResponse {
    run_id: String,
}

#[derive(Debug)]
pub struct CompletedRun {
    pub content: String,
    pub raw_event: Value,
}

impl HermesClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    pub async fn capabilities(&self) -> Result<HermesCapabilities, SearchError> {
        let response = self
            .http
            .get(self.url("/v1/capabilities"))
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let response = checked(response).await?;
        let payload: Value = response.json().await?;
        let features = payload
            .get("features")
            .and_then(Value::as_object)
            .ok_or_else(|| SearchError::HermesProtocol("capabilities.features is missing".to_string()))?;

        Ok(HermesCapabilities {
            run_submission: feature(features, "run_submission"),
            run_events_sse: feature(features, "run_events_sse"),
            run_stop: feature(features, "run_stop"),
        })
    }

    pub async fn require_research_capabilities(&self) -> Result<HermesCapabilities, SearchError> {
        let caps = self.capabilities().await?;
        let mut missing = Vec::new();
        if !caps.run_submission {
            missing.push("run_submission");
        }
        if !caps.run_events_sse {
            missing.push("run_events_sse");
        }
        if !caps.run_stop {
            missing.push("run_stop");
        }
        if !missing.is_empty() {
            return Err(SearchError::HermesProtocol(format!(
                "Hermes is missing required capabilities: {}",
                missing.join(", ")
            )));
        }
        Ok(caps)
    }

    pub async fn start_run(
        &self,
        input: &str,
        instructions: &str,
        session_id: &str,
        idempotency_key: &str,
    ) -> Result<String, SearchError> {
        let request = StartRunRequest {
            input,
            instructions,
            session_id,
        };
        let response = self
            .http
            .post(self.url("/v1/runs"))
            .bearer_auth(&self.api_key)
            .header("Idempotency-Key", idempotency_key)
            .json(&request)
            .send()
            .await?;
        let response = checked(response).await?;
        let ack: StartRunResponse = response.json().await?;
        if ack.run_id.trim().is_empty() {
            return Err(SearchError::HermesProtocol(
                "Hermes returned an empty run_id".to_string(),
            ));
        }
        Ok(ack.run_id)
    }

    pub async fn wait_for_completion(&self, run_id: &str) -> Result<CompletedRun, SearchError> {
        let response = self
            .http
            .get(self.url(&format!("/v1/runs/{run_id}/events")))
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let response = checked(response).await?;

        // The Hermes run event endpoint closes after the terminal event. Keeping the
        // parser here deliberately small makes the bridge independent of Hermes internals;
        // only the public SSE event names and terminal payload are consumed.
        let body = response.text().await?;
        parse_terminal_event(&body)
    }

    pub async fn stop_run(&self, run_id: &str) -> Result<(), SearchError> {
        let response = self
            .http
            .post(self.url(&format!("/v1/runs/{run_id}/stop")))
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        let _ = checked(response).await?;
        Ok(())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn feature(features: &serde_json::Map<String, Value>, name: &str) -> bool {
    features.get(name).and_then(Value::as_bool).unwrap_or(false)
}

async fn checked(response: reqwest::Response) -> Result<reqwest::Response, SearchError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read Hermes error body>".to_string());
    Err(SearchError::HermesHttp {
        status: status.as_u16(),
        body,
    })
}

fn parse_terminal_event(body: &str) -> Result<CompletedRun, SearchError> {
    let normalized = body.replace("\r\n", "\n");
    let mut terminal_error: Option<String> = None;

    for block in normalized.split("\n\n") {
        let mut event_name: Option<&str> = None;
        let mut data_lines = Vec::new();

        for line in block.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event_name = Some(value.trim());
            } else if let Some(value) = line.strip_prefix("data:") {
                data_lines.push(value.trim_start());
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let data = data_lines.join("\n");
        let payload: Value = match serde_json::from_str(&data) {
            Ok(payload) => payload,
            Err(_) => continue,
        };
        let effective_event = event_name
            .or_else(|| payload.get("event").and_then(Value::as_str))
            .unwrap_or_default();

        match effective_event {
            "run.completed" => {
                let content = extract_final_text(&payload).ok_or_else(|| {
                    SearchError::HermesProtocol(
                        "run.completed did not contain a final assistant response".to_string(),
                    )
                })?;
                return Ok(CompletedRun {
                    content,
                    raw_event: payload,
                });
            }
            "run.failed" | "run.interrupted" | "run.cancelled" | "run.stopped" => {
                terminal_error = Some(
                    payload
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or(effective_event)
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    if let Some(error) = terminal_error {
        return Err(SearchError::HermesRun(error));
    }

    Err(SearchError::HermesProtocol(
        "Hermes event stream ended without a terminal run event".to_string(),
    ))
}

fn extract_final_text(payload: &Value) -> Option<String> {
    for key in ["content", "final_response", "output"] {
        if let Some(value) = payload.get(key) {
            if let Some(text) = value_as_text(value) {
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }

    if let Some(messages) = payload.get("messages").and_then(Value::as_array) {
        for message in messages.iter().rev() {
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            if let Some(text) = message.get("content").and_then(value_as_text) {
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }

    None
}

fn value_as_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    let parts = value.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .or_else(|| part.get("content").and_then(Value::as_str))
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::parse_terminal_event;

    #[test]
    fn parses_completed_sse() {
        let body = "event: tool.started\ndata: {\"tool\":\"web_search\"}\n\n\
                    event: run.completed\ndata: {\"event\":\"run.completed\",\"content\":\"answer\"}\n\n";
        let completed = parse_terminal_event(body).expect("completed run");
        assert_eq!(completed.content, "answer");
    }

    #[test]
    fn parses_final_response_fallback() {
        let body = "event: run.completed\ndata: {\"event\":\"run.completed\",\"final_response\":\"answer\"}\n\n";
        let completed = parse_terminal_event(body).expect("completed run");
        assert_eq!(completed.content, "answer");
    }

    #[test]
    fn rejects_failed_sse() {
        let body = "event: run.failed\ndata: {\"error\":\"boom\"}\n\n";
        let error = parse_terminal_event(body).expect_err("failed run");
        assert!(error.to_string().contains("boom"));
    }
}
