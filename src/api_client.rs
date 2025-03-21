use super::{ApiProvider, Config};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Empty API response")]
    EmptyResponse,
    #[error("API request failed: {0}")]
    RequestFailed(String),
    #[error("Response parsing failed: {0}")]
    ParseFailure(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}
use reqwest::blocking::Client;
use serde_json::Value;
use anyhow::Result;

pub trait ApiClient {
    fn generate_comment(&self, system_prompt: &str, diff: &str) -> Result<String>;
}

pub struct OpenAIClient {
    client: Client,
    api_key: String,
    endpoint: String,
    model: String,
}

pub struct ClaudeClient {
    client: Client,
    api_key: String,
    endpoint: String,
    model: String,
}

impl OpenAIClient {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: config.openai_api_key.clone().ok_or(ApiError::ConfigError("Missing OpenAI API key".into()))?,
            endpoint: config.openai_endpoint.clone().unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".into()),
            model: config.openai_model.clone().unwrap_or_else(|| "gpt-4-turbo".into()),
        })
    }
}

impl ClaudeClient {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: config.claude_api_key.clone().ok_or(ApiError::ConfigError("Missing Claude API key".into()))?,
            endpoint: config.claude_endpoint.clone().unwrap_or_else(|| "https://api.anthropic.com/v1/messages".into()),
            model: config.claude_model.clone().unwrap_or_else(|| "claude-3-7-sonnet-20250219".into()),
        })
    }
}

impl ApiClient for OpenAIClient {
    fn generate_comment(&self, system_prompt: &str, diff: &str) -> Result<String> {
        let request_body = json!({
            "model": &self.model,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": format!("Git diff:\n\n{}", diff)
                }
            ],
            "temperature": 0.7
        });

        let response = self.client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .context("Failed to call OpenAI API")?;

        if !response.status().is_success() {
            let error_text = response.text().unwrap_or_else(|_| "Could not read error response".to_string());
            return Err(ApiError::RequestFailed(format!("OpenAI API error: {}", error_text)).into());
        }

        let response_body: Value = response.json()
            .context("Failed to parse OpenAI API response")?;

        response_body["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or(ApiError::EmptyResponse.into())
    }
}

impl ApiClient for ClaudeClient {
    fn generate_comment(&self, system_prompt: &str, diff: &str) -> Result<String> {
        let request_body = json!({
            "model": &self.model,
            "system": system_prompt,
            "messages": [
                {
                    "role": "user",
                    "content": format!("Git diff:\n\n{}", diff)
                }
            ],
            "temperature": 0.7,
            "max_tokens": 4000
        });

        let response = self.client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&request_body)
            .send()
            .context("Failed to call Claude API")?;

        if !response.status().is_success() {
            let error_text = response.text().unwrap_or_else(|_| "Could not read error response".to_string());
            return Err(ApiError::RequestFailed(format!("Claude API error: {}", error_text)).into());
        }

        let response_body: Value = response.json()
            .context("Failed to parse Claude API response")?;

        response_body["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or(ApiError::EmptyResponse.into())
    }
}
