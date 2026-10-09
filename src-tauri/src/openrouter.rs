//! Live, account-filtered OpenRouter access with a fail-closed free-only boundary.
use std::{fmt, time::Duration};

use async_trait::async_trait;
use reqwest::{header, Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::{
    openai_request_body, parse_chat_completion, AttachmentContent, CancellationToken,
    ChatCompletion, ChatCompletionRequest, ChatProvider, ProviderError,
};

pub const BASE_URL: &str = "https://openrouter.ai/api/v1";
const BODY_LIMIT: usize = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FreeModel {
    pub id: String,
    pub name: String,
    pub context_length: u64,
    pub input_modalities: Vec<String>,
    pub supported_parameters: Vec<String>,
    pub reasoning_efforts: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FreeCatalog {
    pub models: Vec<FreeModel>,
    pub fetched_at_ms: i64,
}

fn invalid(message: &str) -> ProviderError {
    ProviderError::InvalidResponse {
        message: message.into(),
    }
}

fn configuration(message: &str) -> ProviderError {
    ProviderError::InvalidConfiguration {
        message: message.into(),
    }
}

fn client() -> Result<Client, ProviderError> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .build()
        .map_err(|_| configuration("Could not construct the OpenRouter HTTPS client"))
}

fn authorization(key: &str) -> Result<header::HeaderValue, ProviderError> {
    if key.trim().is_empty() || key.chars().any(char::is_whitespace) {
        return Err(configuration("OpenRouter API key is empty or invalid"));
    }
    let mut value = header::HeaderValue::from_str(&format!("Bearer {key}"))
        .map_err(|_| configuration("OpenRouter API key is invalid"))?;
    value.set_sensitive(true);
    Ok(value)
}

fn status_error(status: u16) -> ProviderError {
    let message = match status {
        401 | 403 => "OpenRouter authentication or account policy denied this request",
        402 => "OpenRouter free quota is exhausted or this route requires payment; paid fallback is disabled",
        429 => "OpenRouter free rate limit or quota is exhausted; try again later",
        404 | 410 => "The selected OpenRouter model or free route is unavailable",
        400 | 422 => "OpenRouter rejected the requested model capabilities or free-only route",
        502 | 503 => "No available OpenRouter provider could serve this free-only request",
        300..=399 => "OpenRouter returned a redirect; redirects are disabled",
        _ => "OpenRouter request failed; no paid fallback was attempted",
    };
    ProviderError::HttpStatus {
        status,
        body: message.into(),
    }
}

async fn send_bounded(
    request: RequestBuilder,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ProviderError> {
    // Covers headers AND the entire streaming body even on a slow/drip response.
    let operation = async {
        let mut response = request.send().await.map_err(|_| ProviderError::Transport {
            message: "OpenRouter HTTPS request failed".into(),
        })?;
        if !response.status().is_success() {
            return Err(status_error(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > BODY_LIMIT as u64)
        {
            return Err(ProviderError::ResponseTooLarge {
                limit_bytes: BODY_LIMIT,
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ProviderError::Transport {
                message: "Could not read the OpenRouter response".into(),
            })?
        {
            if bytes.len().saturating_add(chunk.len()) > BODY_LIMIT {
                return Err(ProviderError::ResponseTooLarge {
                    limit_bytes: BODY_LIMIT,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ProviderError::Cancelled),
        result = tokio::time::timeout(TIMEOUT, operation) => result.unwrap_or_else(|_| {
            Err(ProviderError::Transport { message: "OpenRouter request timed out after 20 seconds".into() })
        }),
    }
}

pub async fn fetch_catalog(
    api_key: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<FreeCatalog, ProviderError> {
    fetch_with(&client()?, BASE_URL, api_key, cancellation).await
}

async fn fetch_with(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<FreeCatalog, ProviderError> {
    let path = if api_key.is_some() {
        "models/user"
    } else {
        "models"
    };
    let mut request = client.get(format!("{base_url}/{path}"));
    if let Some(key) = api_key {
        request = request.header(header::AUTHORIZATION, authorization(key)?);
    }
    let bytes = send_bounded(request, cancellation).await?;
    let models = parse_free_catalog(catalog_json(&bytes)?)?;
    Ok(FreeCatalog {
        models,
        fetched_at_ms: chrono::Utc::now().timestamp_millis(),
    })
}

// serde_json's default Number uses f64 for decimal literals. Before decoding a
// catalog, retain number tokens as strings: 1e-999 must never turn into 0.0.
// Only context_length is restored to an integer; price strings remain exact.
fn catalog_json(bytes: &[u8]) -> Result<Value, ProviderError> {
    // Validate original JSON grammar before preserving numeric token text. This
    // must not turn invalid unquoted object keys into an accepted catalog.
    serde_json::from_slice::<serde::de::IgnoredAny>(bytes)
        .map_err(|_| invalid("OpenRouter returned an invalid model catalog"))?;
    let mut exact = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                exact.push(bytes[i]);
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    exact.push(b);
                    i += 1;
                    if b == b'\\' && i < bytes.len() {
                        exact.push(bytes[i]);
                        i += 1;
                    } else if b == b'"' {
                        break;
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    i += 1;
                }
                let token = &bytes[start..i];
                if !decimal_syntax(token) {
                    return Err(invalid("OpenRouter catalog contains an invalid number"));
                }
                exact.push(b'"');
                exact.extend_from_slice(token);
                exact.push(b'"');
            }
            b => {
                exact.push(b);
                i += 1;
            }
        }
    }
    let mut value: Value = serde_json::from_slice(&exact)
        .map_err(|_| invalid("OpenRouter returned an invalid model catalog"))?;
    if let Some(rows) = value.get_mut("data").and_then(Value::as_array_mut) {
        for row in rows {
            if let Some(length) = row
                .get("context_length")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok())
            {
                row["context_length"] = Value::from(length);
            }
        }
    }
    Ok(value)
}

fn decimal_syntax(bytes: &[u8]) -> bool {
    let mut i = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == bytes.len()
}

fn zero_price(price: &Value) -> bool {
    let text = match price {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return false,
    };
    if text.starts_with('-') || !decimal_syntax(text.as_bytes()) {
        return false;
    }
    text.split(['e', 'E'])
        .next()
        .is_some_and(|mantissa| mantissa.bytes().all(|b| b == b'0' || b == b'.'))
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a decoded catalog, preserving its order. Callers decoding raw JSON must
/// preserve decimal prices (the HTTP path uses catalog_json for this reason).
pub fn parse_free_catalog(value: Value) -> Result<Vec<FreeModel>, ProviderError> {
    if value.get("error").is_some() {
        return Err(invalid("OpenRouter model catalog reported an error"));
    }
    let rows = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("OpenRouter model catalog is missing its data array"))?;
    let mut models = Vec::new();
    for row in rows {
        let Some(pricing) = row.get("pricing").and_then(Value::as_object) else {
            continue;
        };
        if !pricing.get("prompt").is_some_and(zero_price)
            || !pricing.get("completion").is_some_and(zero_price)
            || !pricing.values().all(zero_price)
        {
            continue;
        }
        let inputs = strings(&row["architecture"]["input_modalities"]);
        // This adapter certifies text replies only. Media-generation endpoints
        // may advertise zero token prices yet bill per song/image/second through
        // a separate pricing schema. They require their own verified cost gate.
        let text_output_only = row["architecture"]["output_modalities"]
            .as_array()
            .is_some_and(|outputs| outputs.len() == 1 && outputs[0].as_str() == Some("text"));
        if !inputs.iter().any(|s| s == "text") || !text_output_only {
            continue;
        }
        let (Some(id), Some(name), Some(context_length)) = (
            row["id"].as_str().filter(|s| !s.trim().is_empty()),
            row["name"].as_str().filter(|s| !s.trim().is_empty()),
            row["context_length"].as_u64(),
        ) else {
            continue;
        };
        models.push(FreeModel {
            id: id.into(),
            name: name.into(),
            context_length,
            input_modalities: inputs,
            supported_parameters: strings(&row["supported_parameters"]),
            // A generic "reasoning" parameter is NOT an enumeration of efforts.
            reasoning_efforts: strings(&row["supported_reasoning_efforts"]),
        });
    }
    Ok(models)
}

pub struct OpenRouterProvider {
    client: Client,
    api_key: header::HeaderValue,
    model: String,
    reasoning_effort: Option<String>,
    #[cfg(test)]
    test_base_url: Option<String>,
}

impl fmt::Debug for OpenRouterProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenRouterProvider")
            .field("api_key", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl OpenRouterProvider {
    pub fn new(
        api_key: String,
        model: String,
        reasoning_effort: Option<String>,
    ) -> Result<Self, ProviderError> {
        if model.trim().is_empty() {
            return Err(configuration("Select an OpenRouter free model"));
        }
        if reasoning_effort
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
        {
            return Err(configuration("Reasoning effort must be explicit or unset"));
        }
        Ok(Self {
            client: client()?,
            api_key: authorization(&api_key)?,
            model,
            reasoning_effort,
            #[cfg(test)]
            test_base_url: None,
        })
    }

    fn base_url(&self) -> &str {
        #[cfg(test)]
        if let Some(url) = &self.test_base_url {
            return url;
        }
        BASE_URL
    }

    fn request_body(
        &self,
        mut request: ChatCompletionRequest,
        model: &FreeModel,
    ) -> Result<Value, ProviderError> {
        if request.model != self.model {
            return Err(configuration(
                "OpenRouter request model does not match the selected model",
            ));
        }
        for message in &request.messages {
            for attachment in &message.attachments {
                match attachment {
                    AttachmentContent::Image { .. }
                        if !model.input_modalities.iter().any(|s| s == "image") =>
                    {
                        return Err(ProviderError::Unsupported {
                            capability: "image input for this OpenRouter free model".into(),
                        });
                    }
                    AttachmentContent::File { .. } => {
                        return Err(ProviderError::Unsupported {
                            capability:
                                "raw file input; use native extraction before OpenRouter chat"
                                    .into(),
                        })
                    }
                    _ => {}
                }
            }
        }
        if !model.supported_parameters.iter().any(|s| s == "tools") {
            // Preserve historical assistant calls and tool results verbatim. Only
            // the ability to request NEW calls is omitted for plain chat models.
            request.tools.clear();
        }
        let mut body = openai_request_body(&request)?;
        body["provider"] = json!({
            "max_price": { "prompt": 0, "completion": 0, "request": 0, "image": 0 },
            "allow_fallbacks": false, "require_parameters": true
        });
        body["plugins"] = json!([]);
        if let Some(effort) = &self.reasoning_effort {
            if !model.reasoning_efforts.contains(effort) {
                return Err(configuration(
                    "The live OpenRouter catalog does not explicitly support this reasoning effort",
                ));
            }
            body["reasoning"] = json!({ "effort": effort });
        }
        Ok(body)
    }
}

#[async_trait]
impl ChatProvider for OpenRouterProvider {
    async fn complete(
        &self,
        request: ChatCompletionRequest,
        cancellation: &CancellationToken,
    ) -> Result<ChatCompletion, ProviderError> {
        // Mandatory authenticated fresh read before EVERY completion. Never fall
        // back to a public catalog, cached selection, another model, or paid route.
        let catalog_request = self
            .client
            .get(format!("{}/models/user", self.base_url()))
            .header(header::AUTHORIZATION, self.api_key.clone());
        let bytes = send_bounded(catalog_request, cancellation).await?;
        let catalog = parse_free_catalog(catalog_json(&bytes)?)?;
        let selected = catalog.iter().find(|m| m.id == self.model).ok_or_else(|| configuration(
            "The selected OpenRouter model is no longer verified free or available for this account; refresh the catalog and select a free model"
        ))?;
        let body = self.request_body(request, selected)?;
        let response = send_bounded(
            self.client
                .post(format!("{}/chat/completions", self.base_url()))
                .header(header::AUTHORIZATION, self.api_key.clone())
                .json(&body),
            cancellation,
        )
        .await?;
        let value: Value = serde_json::from_slice(&response)
            .map_err(|_| invalid("OpenRouter returned an invalid chat response"))?;
        if let Some(error) = value.get("error") {
            return Err(error
                .get("code")
                .and_then(Value::as_u64)
                .and_then(|c| u16::try_from(c).ok())
                .map(status_error)
                .unwrap_or_else(|| {
                    invalid("OpenRouter reported a chat error; no paid fallback was attempted")
                }));
        }
        // Keep the actual response model, including when it differs from selection.
        parse_chat_completion(&response)
            .map_err(|_| invalid("OpenRouter returned an invalid chat completion"))
    }
}

#[cfg(test)]
mod tests;
