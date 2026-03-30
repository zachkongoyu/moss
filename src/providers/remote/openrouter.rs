use async_trait::async_trait;
use serde_json::Value;
use serde_json::json;
use crate::providers::{LlmProvider, Message};
use serde::Serialize;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use std::time::Duration;
use std::path::Path;
use std::fs;

fn load_dotenv(dotenv_path: Option<&Path>) {
    let path = dotenv_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| Path::new(".env").to_path_buf());
    if !path.exists() {
        return;
    }
    if let Ok(text) = fs::read_to_string(&path) {
        for raw_line in text.split_terminator('\n') {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || !line.contains('=') {
                continue;
            }
            let mut parts = line.splitn(2, '=');
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                let key: &str = k.trim();
                let value = v.trim().trim_matches('"').trim_matches('\'');
                if !key.is_empty() && std::env::var_os(key).is_none() {
                    std::env::set_var(key, value);
                }
            }
        }
    }
}

pub struct OpenRouter {
    base_url: String,
    api_key: String,
    app_url: Option<String>,
    app_name: Option<String>,
    model: String,
    timeout: Duration,
}

impl OpenRouter {
    pub fn new(model: Option<String>, api_key: Option<String>) -> Result<Self, String> {
        load_dotenv(None);
        let api = api_key.or_else(|| std::env::var("OPENROUTER_API_KEY").ok());
        let api = api.ok_or_else(|| "OpenRouter API key is required (OPENROUTER_API_KEY)".to_string())?;
        let base = std::env::var("OPENROUTER_BASE_URL").unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
        let app_url = std::env::var("OPENROUTER_APP_URL").ok();
        let app_name = std::env::var("OPENROUTER_APP_NAME").ok();
        let model = model.unwrap_or_else(|| "google/gemini-3-flash-preview".to_string());
        Ok(Self {
            base_url: base,
            api_key: api,
            app_url,
            app_name,
            model,
            timeout: Duration::from_secs(60),
        })
    }

    fn build_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        let auth = HeaderValue::from_str(&format!("Bearer {}", self.api_key))
            .expect("invalid authorization header");
        headers.insert("Authorization", auth);

        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let url = self.app_url.as_ref().unwrap();
        let hv = HeaderValue::from_str(url).expect("invalid OPENROUTER_APP_URL header");
        headers.insert("Referer", hv);

        let name = self.app_name.as_ref().unwrap();
        let hv2 = HeaderValue::from_str(name).expect("invalid OPENROUTER_APP_NAME header");
        headers.insert("X-OpenRouter-Title", hv2);

        headers
    }

    fn extract_content_text(value: &Value) -> String {
        match value {
            Value::String(s) => s.clone(),
            Value::Array(arr) => {
                let mut chunks = String::new();
                for item in arr {
                    if let Value::Object(map) = item {
                                let val = map.get("text").or_else(|| map.get("content")).expect("missing text/content in message item");
                                chunks.push_str(&val.to_string().trim_matches('"').to_string());
                    }
                }
                chunks
            }
            other => other.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenRouter {
    async fn complete_chat(&self, messages: Vec<Message>) -> Value {
        let client = reqwest::Client::builder().timeout(self.timeout).build().expect("failed to build reqwest client");

        let msgs = serde_json::to_value(&messages).expect("serialize messages error");

        let body = json!({
            "model": self.model,
            "messages": msgs,
        });

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let headers = self.build_headers();

        let res = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .expect("request error");

        let status = res.status();
        let text = res.text().await.expect("read body error");

        let mut json: Value = serde_json::from_str(&text).expect("json parse error");

        if !status.is_success() {
            panic!("openrouter error {}: {}", status, json);
        }

        // extract choices[0].message.content using typed accessors
        let txt = json.pointer_mut("/choices/0/message/content")
            .map(|v| v.take()) // Moves the value out of the JSON map
            .and_then(|v| v.as_str().map(|s| s.to_string())) // Only one string copy here
            .expect("OpenRouter response was missing content");

        Value::String(txt)
    }
}
