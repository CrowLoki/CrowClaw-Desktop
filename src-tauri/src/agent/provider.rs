use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{header, Client, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{
    AssistantToolCall, CancellationToken, ChatCompletion, ChatCompletionRequest, ChatMessage,
    ChatRole, ProviderConfig, ProviderError, TokenUsage,
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealthState {
    Healthy,
    Reachable,
    Unavailable,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    pub state: ProviderHealthState,
    pub endpoint: String,
    pub latency_ms: u64,
    pub model_listing_supported: bool,
    pub model_count: usize,
    pub detail: String,
}

#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn complete(
        &self,
        request: ChatCompletionRequest,
        cancellation: &CancellationToken,
    ) -> Result<ChatCompletion, ProviderError>;
}

#[derive(Clone, Debug)]
pub struct OpenAiCompatibleClient {
    config: ProviderConfig,
    client: Client,
}

impl OpenAiCompatibleClient {
    pub fn new(config: ProviderConfig) -> Result<Self, ProviderError> {
        config.normalized_base_url()?;
        let client = Client::builder()
            .connect_timeout(Duration::from_millis(config.request_timeout_ms))
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(|error| ProviderError::InvalidConfiguration {
                message: format!("could not construct HTTP client: {error}"),
            })?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub async fn list_models(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<ProviderModel>, ProviderError> {
        let endpoint = self.config.endpoint("models")?;
        let response = self
            .send(self.authorized(self.client.get(endpoint)), cancellation)
            .await?;
        let status = response.status();
        let body = read_bounded(response, self.config.max_response_bytes, cancellation).await?;

        if matches!(
            status,
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ) {
            return Err(ProviderError::Unsupported {
                capability: "model listing".into(),
            });
        }
        if !status.is_success() {
            return Err(http_status_error(status, &body));
        }

        let response: RawModelList =
            serde_json::from_slice(&body).map_err(|error| ProviderError::InvalidResponse {
                message: format!("invalid model-list response: {error}"),
            })?;
        let mut models = response
            .data
            .into_iter()
            .filter(|model| !model.id.trim().is_empty())
            .map(|model| ProviderModel {
                id: model.id,
                owned_by: model.owned_by,
            })
            .collect::<Vec<_>>();
        models.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(models)
    }

    pub async fn health(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<ProviderHealth, ProviderError> {
        let endpoint = self.config.endpoint("models")?;
        let started = Instant::now();
        let response = match self
            .send(
                self.authorized(self.client.get(endpoint.clone())),
                cancellation,
            )
            .await
        {
            Ok(response) => response,
            Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
            Err(error) => {
                return Ok(ProviderHealth {
                    state: ProviderHealthState::Unavailable,
                    endpoint,
                    latency_ms: elapsed_millis(started),
                    model_listing_supported: false,
                    model_count: 0,
                    detail: error.to_string(),
                })
            }
        };

        let status = response.status();
        let body = match read_bounded(response, self.config.max_response_bytes, cancellation).await
        {
            Ok(body) => body,
            Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
            Err(error) => {
                return Ok(ProviderHealth {
                    state: ProviderHealthState::Reachable,
                    endpoint,
                    latency_ms: elapsed_millis(started),
                    model_listing_supported: false,
                    model_count: 0,
                    detail: error.to_string(),
                })
            }
        };

        if status.is_success() {
            match serde_json::from_slice::<RawModelList>(&body) {
                Ok(models) => Ok(ProviderHealth {
                    state: ProviderHealthState::Healthy,
                    endpoint,
                    latency_ms: elapsed_millis(started),
                    model_listing_supported: true,
                    model_count: models.data.len(),
                    detail: "OpenAI-compatible model endpoint is healthy".into(),
                }),
                Err(error) => Ok(ProviderHealth {
                    state: ProviderHealthState::Reachable,
                    endpoint,
                    latency_ms: elapsed_millis(started),
                    model_listing_supported: false,
                    model_count: 0,
                    detail: format!("endpoint responded but model data was invalid: {error}"),
                }),
            }
        } else {
            Ok(ProviderHealth {
                state: ProviderHealthState::Reachable,
                endpoint,
                latency_ms: elapsed_millis(started),
                model_listing_supported: !matches!(
                    status,
                    StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
                ),
                model_count: 0,
                detail: format!(
                    "endpoint responded with HTTP {}: {}",
                    status.as_u16(),
                    bounded_text(&body)
                ),
            })
        }
    }

    async fn chat_completion(
        &self,
        request: ChatCompletionRequest,
        cancellation: &CancellationToken,
    ) -> Result<ChatCompletion, ProviderError> {
        if request.model.trim().is_empty() {
            return Err(ProviderError::InvalidConfiguration {
                message: "chat completion requires a model".into(),
            });
        }
        if request.messages.is_empty() {
            return Err(ProviderError::InvalidConfiguration {
                message: "chat completion requires at least one message".into(),
            });
        }

        let endpoint = self.config.endpoint("chat/completions")?;
        let payload = openai_request_body(&request)?;
        let response = self
            .send(
                self.authorized(self.client.post(endpoint)).json(&payload),
                cancellation,
            )
            .await?;
        let status = response.status();
        let body = read_bounded(response, self.config.max_response_bytes, cancellation).await?;
        if !status.is_success() {
            return Err(http_status_error(status, &body));
        }

        parse_chat_completion(&body)
    }

    fn authorized(&self, request: RequestBuilder) -> RequestBuilder {
        match self
            .config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            Some(api_key) => request.header(header::AUTHORIZATION, format!("Bearer {api_key}")),
            None => request,
        }
    }

    async fn send(
        &self,
        request: RequestBuilder,
        cancellation: &CancellationToken,
    ) -> Result<Response, ProviderError> {
        tokio::select! {
            _ = cancellation.cancelled() => Err(ProviderError::Cancelled),
            result = request.send() => result.map_err(|error| ProviderError::Transport {
                message: error.to_string(),
            }),
        }
    }
}

#[async_trait]
impl ChatProvider for OpenAiCompatibleClient {
    async fn complete(
        &self,
        request: ChatCompletionRequest,
        cancellation: &CancellationToken,
    ) -> Result<ChatCompletion, ProviderError> {
        self.chat_completion(request, cancellation).await
    }
}

fn openai_request_body(request: &ChatCompletionRequest) -> Result<Value, ProviderError> {
    let messages = request
        .messages
        .iter()
        .map(openai_message)
        .collect::<Result<Vec<_>, _>>()?;
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                }
            })
        })
        .collect::<Vec<_>>();

    let mut payload = Map::new();
    payload.insert("model".into(), Value::String(request.model.clone()));
    payload.insert("messages".into(), Value::Array(messages));
    payload.insert("stream".into(), Value::Bool(false));
    if !tools.is_empty() {
        payload.insert("tools".into(), Value::Array(tools));
        payload.insert("tool_choice".into(), Value::String("auto".into()));
    }
    if let Some(temperature) = request.temperature {
        let value = serde_json::Number::from_f64(temperature as f64).ok_or_else(|| {
            ProviderError::InvalidConfiguration {
                message: "temperature must be finite".into(),
            }
        })?;
        payload.insert("temperature".into(), Value::Number(value));
    }
    if let Some(max_tokens) = request.max_tokens {
        payload.insert("max_tokens".into(), Value::from(max_tokens));
    }
    Ok(Value::Object(payload))
}

fn openai_message(message: &ChatMessage) -> Result<Value, ProviderError> {
    let mut output = Map::new();
    output.insert(
        "role".into(),
        Value::String(
            match message.role {
                ChatRole::System => "system",
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::Tool => "tool",
            }
            .into(),
        ),
    );
    output.insert(
        "content".into(),
        message
            .content
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    if !message.tool_calls.is_empty() {
        let calls = message
            .tool_calls
            .iter()
            .map(|call| {
                let arguments = serde_json::to_string(&call.arguments).map_err(|error| {
                    ProviderError::InvalidConfiguration {
                        message: format!("could not encode tool-call arguments: {error}"),
                    }
                })?;
                Ok(json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": arguments,
                    }
                }))
            })
            .collect::<Result<Vec<_>, ProviderError>>()?;
        output.insert("tool_calls".into(), Value::Array(calls));
    }
    if let Some(tool_call_id) = &message.tool_call_id {
        output.insert("tool_call_id".into(), Value::String(tool_call_id.clone()));
    }
    if let Some(name) = &message.name {
        output.insert("name".into(), Value::String(name.clone()));
    }
    Ok(Value::Object(output))
}

fn parse_chat_completion(body: &[u8]) -> Result<ChatCompletion, ProviderError> {
    let raw: RawChatCompletion =
        serde_json::from_slice(body).map_err(|error| ProviderError::InvalidResponse {
            message: format!("invalid chat-completion response: {error}"),
        })?;
    let choice = raw
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::InvalidResponse {
            message: "chat-completion response contained no choices".into(),
        })?;
    let mut tool_calls = Vec::with_capacity(choice.message.tool_calls.len());
    for call in choice.message.tool_calls {
        if call.id.trim().is_empty() || call.function.name.trim().is_empty() {
            return Err(ProviderError::InvalidResponse {
                message: "tool call is missing its id or function name".into(),
            });
        }
        let arguments = match call.function.arguments {
            RawArguments::String(arguments) => {
                serde_json::from_str(&arguments).map_err(|error| {
                    ProviderError::InvalidResponse {
                        message: format!(
                            "tool call '{}' returned invalid JSON arguments: {error}",
                            call.function.name
                        ),
                    }
                })?
            }
            RawArguments::Value(arguments) => arguments,
        };
        tool_calls.push(AssistantToolCall {
            id: call.id,
            name: call.function.name,
            arguments,
        });
    }
    Ok(ChatCompletion {
        id: raw.id,
        model: raw.model,
        message: ChatMessage::assistant_with_tool_calls(choice.message.content, tool_calls),
        finish_reason: choice.finish_reason,
        usage: raw.usage,
    })
}

async fn read_bounded(
    mut response: Response,
    limit: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ProviderError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ProviderError::ResponseTooLarge { limit_bytes: limit });
    }

    let mut body = Vec::with_capacity(min_usize(limit, 64 * 1024));
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
            result = response.chunk() => result.map_err(|error| ProviderError::Transport {
                message: format!("could not read provider response: {error}"),
            })?,
        };
        let Some(chunk) = chunk else {
            return Ok(body);
        };
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(ProviderError::ResponseTooLarge { limit_bytes: limit });
        }
        body.extend_from_slice(&chunk);
    }
}

fn http_status_error(status: StatusCode, body: &[u8]) -> ProviderError {
    ProviderError::HttpStatus {
        status: status.as_u16(),
        body: bounded_text(body),
    }
}

fn bounded_text(body: &[u8]) -> String {
    const ERROR_TEXT_LIMIT: usize = 4 * 1024;
    let prefix = &body[..min_usize(body.len(), ERROR_TEXT_LIMIT)];
    let mut text = String::from_utf8_lossy(prefix).into_owned();
    if body.len() > ERROR_TEXT_LIMIT {
        text.push_str("…[truncated]");
    }
    text
}

fn min_usize(left: usize, right: usize) -> usize {
    if left < right {
        left
    } else {
        right
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[derive(Deserialize)]
struct RawModelList {
    #[serde(default)]
    data: Vec<RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    id: String,
    #[serde(default)]
    owned_by: Option<String>,
}

#[derive(Deserialize)]
struct RawChatCompletion {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    choices: Vec<RawChoice>,
    #[serde(default)]
    usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
struct RawChoice {
    message: RawAssistantMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct RawAssistantMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<RawToolCall>,
}

#[derive(Deserialize)]
struct RawToolCall {
    id: String,
    function: RawFunctionCall,
}

#[derive(Deserialize)]
struct RawFunctionCall {
    name: String,
    arguments: RawArguments,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawArguments {
    String(String),
    Value(Value),
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    use super::{ChatProvider, OpenAiCompatibleClient, ProviderHealthState};
    use crate::agent::{CancellationToken, ChatCompletionRequest, ChatMessage, ProviderConfig};

    #[tokio::test]
    async fn lists_models_and_reports_healthy_local_endpoint() {
        let (base_url, first_request) = mock_once(
            200,
            r#"{"data":[{"id":"zeta"},{"id":"alpha","owned_by":"local"}]}"#,
        )
        .await;
        let client = OpenAiCompatibleClient::new(ProviderConfig::custom(base_url)).unwrap();
        let models = client.list_models(&CancellationToken::new()).await.unwrap();
        assert_eq!(models[0].id, "alpha");
        assert!(first_request.await.unwrap().starts_with("GET /v1/models "));

        let (base_url, _) = mock_once(200, r#"{"data":[{"id":"local-model"}]}"#).await;
        let client = OpenAiCompatibleClient::new(ProviderConfig::custom(base_url)).unwrap();
        let health = client.health(&CancellationToken::new()).await.unwrap();
        assert_eq!(health.state, ProviderHealthState::Healthy);
        assert_eq!(health.model_count, 1);
    }

    #[tokio::test]
    async fn sends_chat_completion_and_parses_tool_arguments() {
        let response = r#"{
            "id":"chat-1",
            "model":"local-model",
            "choices":[{
                "finish_reason":"tool_calls",
                "message":{
                    "content":null,
                    "tool_calls":[{
                        "id":"call-1",
                        "type":"function",
                        "function":{"name":"read_text_file","arguments":"{\"path\":\"C:/fixture.txt\"}"}
                    }]
                }
            }]
        }"#;
        let (base_url, received) = mock_once(200, response).await;
        let client = OpenAiCompatibleClient::new(ProviderConfig::custom(base_url)).unwrap();
        let completion = client
            .complete(
                ChatCompletionRequest {
                    model: "local-model".into(),
                    messages: vec![ChatMessage::user("inspect the file")],
                    tools: Vec::new(),
                    temperature: None,
                    max_tokens: Some(100),
                },
                &CancellationToken::new(),
            )
            .await
            .unwrap();

        assert_eq!(completion.message.tool_calls[0].name, "read_text_file");
        assert_eq!(
            completion.message.tool_calls[0].arguments["path"],
            "C:/fixture.txt"
        );
        let request = received.await.unwrap();
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request.contains("\"model\":\"local-model\""));
    }

    #[tokio::test]
    async fn cancellation_prevents_request_completion() {
        let token = CancellationToken::new();
        token.cancel();
        let client =
            OpenAiCompatibleClient::new(ProviderConfig::custom("http://127.0.0.1:9/v1")).unwrap();
        let error = client.list_models(&token).await.unwrap_err();
        assert!(matches!(error, crate::agent::ProviderError::Cancelled));
    }

    async fn mock_once(status: u16, body: &'static str) -> (String, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let count = stream.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length: ")
                                .or_else(|| line.strip_prefix("Content-Length: "))
                        })
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    let header_end = request
                        .windows(4)
                        .position(|window| window == b"\r\n\r\n")
                        .unwrap()
                        + 4;
                    if request.len() >= header_end + content_length {
                        break;
                    }
                }
            }
            let request_text = String::from_utf8_lossy(&request).into_owned();
            let _ = sender.send(request_text);
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{address}/v1"), receiver)
    }
}
