use super::{
    protocol,
    service::{provider_failure, read_body, MembershipService},
    MembershipSelection,
};
use crate::agent::{
    AssistantToolCall, CancellationToken, ChatCompletion, ChatCompletionRequest, ChatMessage,
    ChatProvider, ChatRole, ProviderError, ProviderTurnContext, TokenUsage,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
pub struct MembershipProvider {
    service: Arc<MembershipService>,
    selection: MembershipSelection,
    session: CancellationToken,
    session_version: u32,
}
impl MembershipProvider {
    pub fn new(
        service: Arc<MembershipService>,
        selection: MembershipSelection,
    ) -> Result<Self, String> {
        let account = service
            .storage
            .membership_account(&selection.account_id)
            .map_err(|e| e.to_string())?;
        if !account.has_credentials {
            return Err("Reconnect this saved ChatGPT account".into());
        }
        let session = service.session_cancellation(&selection.account_id)?;
        Ok(Self {
            service,
            selection,
            session,
            session_version: account.session_version,
        })
    }
    fn check_connection(&self) -> Result<(), ProviderError> {
        let account = self
            .service
            .storage
            .membership_account(&self.selection.account_id)
            .map_err(|_| invalid("Saved account is unavailable"))?;
        if !account.has_credentials || account.session_version != self.session_version {
            return Err(ProviderError::Cancelled);
        }
        Ok(())
    }
    async fn run(
        &self,
        request: ChatCompletionRequest,
        cancellation: &CancellationToken,
    ) -> Result<ChatCompletion, ProviderError> {
        if cancellation.is_cancelled() || self.session.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let payload = request_body(&request, &self.selection)?;
        let (credentials, _) = tokio::select! {
            _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
            _ = self.session.cancelled() => return Err(ProviderError::Cancelled),
            result = self.service.credentials(&self.selection.account_id,&self.session) => result.map_err(|message|ProviderError::InvalidConfiguration{message})?,
        };
        self.check_connection()?;
        let request = self
            .service
            .client
            .post(format!("{}/responses", protocol::RESOURCE))
            .bearer_auth(&credentials.access_token)
            .json(&payload);
        let mut check = tokio::time::interval(Duration::from_millis(100));
        let response = self.service.send(request, &self.session);
        tokio::pin!(response);
        let mut response = loop {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
                _ = self.session.cancelled() => return Err(ProviderError::Cancelled),
                _ = check.tick() => self.check_connection()?,
                result = &mut response => break result.map_err(|message|ProviderError::Transport{message})?,
            }
        };
        let status = response.status();
        if !status.is_success() {
            let bytes = tokio::select! {
                _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
                result = read_body(response,&self.session,1024*1024) => result.map_err(|message|ProviderError::Transport{message})?,
            };
            return Err(ProviderError::HttpStatus {
                status: status.as_u16(),
                body: provider_failure(status.as_u16(), &bytes),
            });
        }
        if !stream_content_type_supported(response.headers().get(reqwest::header::CONTENT_TYPE)) {
            // Classify unexpected replies without exposing arbitrary response
            // bodies, account details, credentials or server-supplied headers.
            let media = match response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
            {
                Some("application/json") => "JSON",
                Some("text/html") => "HTML",
                Some("text/plain") => "plain text",
                None => "missing content type",
                _ => "other content type",
            };
            let bytes = tokio::select! {
                _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
                result = read_body(response,&self.session,RESPONSE_LIMIT) => result.map_err(|message|ProviderError::Transport{message})?,
            };
            return Err(invalid(&unexpected_response(
                status.as_u16(),
                media,
                &bytes,
            )));
        }
        let mut decoder = ResponseStream::new(self.selection.clone());
        loop {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(ProviderError::Cancelled),
                _ = self.session.cancelled() => return Err(ProviderError::Cancelled),
                _ = check.tick() => self.check_connection()?,
                result = response.chunk() => match result.map_err(|_|ProviderError::Transport{message:"ChatGPT response was interrupted".into()})? {
                    Some(bytes) => if let Some(completion) = decoder.push(&bytes)? { self.check_connection()?; return Ok(completion); },
                    None => { self.check_connection()?; return decoder.finish(); },
                }
            }
        }
    }
}
#[async_trait]
impl ChatProvider for MembershipProvider {
    async fn complete(
        &self,
        request: ChatCompletionRequest,
        cancellation: &CancellationToken,
    ) -> Result<ChatCompletion, ProviderError> {
        self.run(request, cancellation).await
    }
}

fn invalid(message: &str) -> ProviderError {
    ProviderError::InvalidResponse {
        message: message.into(),
    }
}
fn stream_content_type_supported(value: Option<&reqwest::header::HeaderValue>) -> bool {
    // The live membership endpoint can omit Content-Type while returning SSE.
    // A missing advisory header is not failure: the bounded stream decoder must
    // still validate the actual events and require response.completed. Explicit
    // non-SSE types remain errors; never fall back to accepting a JSON response.
    value.is_none_or(|value| {
        value
            .to_str()
            .ok()
            .and_then(|value| value.split(';').next())
            .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
    })
}
fn unexpected_response(status: u16, media: &str, bytes: &[u8]) -> String {
    let shape = if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        if value.get("error").is_some_and(|error| !error.is_null()) {
            "JSON error"
        } else if value["object"] == "response" {
            "JSON response object"
        } else {
            "other JSON"
        }
    } else if bytes.starts_with(b"event:") || bytes.starts_with(b"data:") {
        "event-stream body"
    } else {
        "unrecognized body"
    };
    format!("ChatGPT did not return the required response stream (HTTP {status}; {media}; {shape})")
}
fn request_body(
    request: &ChatCompletionRequest,
    selection: &MembershipSelection,
) -> Result<Value, ProviderError> {
    if request.model != selection.model || request.messages.is_empty() {
        return Err(ProviderError::InvalidConfiguration {
            message: "Membership request does not match its selected account model".into(),
        });
    }
    let mut input = Vec::new();
    for message in &request.messages {
        if message.role == ChatRole::Assistant {
            if let Some(context) = &message.provider_context {
                if context.provider != "chatgpt"
                    || context.account_id != selection.account_id
                    || context.model != selection.model
                {
                    return Err(ProviderError::InvalidConfiguration{message:"Provider reasoning context belongs to a different account or model; start a new task".into()});
                }
                // Reuse validated output, including encrypted reasoning, for this
                // same native task. No previous-response/server-storage dependency.
                input.extend(context.items.iter().cloned());
                continue;
            }
        }
        match message.role {
            ChatRole::Tool => input.push(json!({"type":"function_call_output","call_id":message.tool_call_id.as_deref().filter(|id|!id.is_empty()).ok_or_else(||invalid("Tool result has no original call identifier"))?,"output":message.content.as_deref().unwrap_or("")})),
            _ => {
                if let Some(text) = &message.content {
                    let role = match message.role {ChatRole::System=>"developer",ChatRole::User=>"user",ChatRole::Assistant=>"assistant",ChatRole::Tool=>unreachable!()};
                    input.push(json!({"role":role,"content":text}));
                }
                for call in &message.tool_calls {
                    input.push(json!({"type":"function_call","call_id":call.id,"name":call.name,"arguments":serde_json::to_string(&call.arguments).map_err(|_|invalid("Could not serialize a previous tool call"))?}));
                }
            }
        }
    }
    let mut payload = json!({"model":selection.model,"input":input,"store":false,"stream":true});
    if let Some(effort) = &selection.reasoning_effort {
        payload["reasoning"] = json!({"effort":effort});
    }
    if !request.tools.is_empty() {
        payload["tools"] = Value::Array(request.tools.iter().map(|tool|json!({"type":"function","name":tool.name,"description":tool.description,"parameters":tool.parameters,"strict":false})).collect());
        payload["tool_choice"] = json!("auto");
    }
    if let Some(limit) = request.max_tokens {
        payload["max_output_tokens"] = json!(limit);
    }
    if let Some(temperature) = request.temperature {
        if !temperature.is_finite() {
            return Err(invalid("Request temperature is invalid"));
        }
        payload["temperature"] = json!(temperature);
    }
    Ok(payload)
}

struct ResponseStream {
    selection: MembershipSelection,
    buffer: Vec<u8>,
    data: Vec<String>,
    consumed: usize,
    terminal: bool,
    completed_items: BTreeMap<usize, Value>,
}
impl ResponseStream {
    fn new(selection: MembershipSelection) -> Self {
        Self {
            selection,
            buffer: Vec::new(),
            data: Vec::new(),
            consumed: 0,
            terminal: false,
            completed_items: BTreeMap::new(),
        }
    }
    fn push(&mut self, bytes: &[u8]) -> Result<Option<ChatCompletion>, ProviderError> {
        if self.terminal {
            return Err(invalid("Data arrived after response completion"));
        }
        self.consumed = self.consumed.saturating_add(bytes.len());
        if self.consumed > RESPONSE_LIMIT {
            return Err(ProviderError::ResponseTooLarge {
                limit_bytes: RESPONSE_LIMIT,
            });
        }
        self.buffer.extend_from_slice(bytes);
        self.drain_events()
    }
    fn drain_events(&mut self) -> Result<Option<ChatCompletion>, ProviderError> {
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let mut line = self.buffer.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = std::str::from_utf8(&line)
                .map_err(|_| invalid("Response stream contains invalid text"))?;
            if line.is_empty() {
                if let Some(completion) = self.event()? {
                    return Ok(Some(completion));
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data
                    .push(data.strip_prefix(' ').unwrap_or(data).into());
            }
        }
        Ok(None)
    }
    fn event(&mut self) -> Result<Option<ChatCompletion>, ProviderError> {
        if self.data.is_empty() {
            return Ok(None);
        }
        let data = std::mem::take(&mut self.data).join("\n");
        if data == "[DONE]" {
            return Err(invalid("Response ended without a completed result"));
        }
        let event: Value =
            serde_json::from_str(&data).map_err(|_| invalid("Response stream event is invalid"))?;
        match event.get("type").and_then(|v| v.as_str()) {
            Some("response.output_item.done") => {
                let index = event["output_index"]
                    .as_u64()
                    .filter(|index| *index < 1024)
                    .ok_or_else(|| invalid("Completed output item index is invalid"))?
                    as usize;
                let item = event
                    .get("item")
                    .filter(|item| item.is_object())
                    .ok_or_else(|| invalid("Completed output item is invalid"))?;
                if self.completed_items.insert(index, item.clone()).is_some() {
                    return Err(invalid("Response repeats a completed output item"));
                }
                Ok(None)
            }
            Some("response.completed") => {
                let mut response = event["response"].clone();
                // Some streaming envelopes omit the already-emitted output from
                // their terminal snapshot. Retain only complete item events,
                // never partial text/argument deltas, and still require the
                // successful response-level terminal event below.
                if response
                    .get("output")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
                    && !self.completed_items.is_empty()
                {
                    if self
                        .completed_items
                        .keys()
                        .copied()
                        .ne(0..self.completed_items.len())
                    {
                        return Err(invalid("Response is missing completed output items"));
                    }
                    response["output"] = Value::Array(
                        std::mem::take(&mut self.completed_items)
                            .into_values()
                            .collect(),
                    );
                }
                let completion = completed_response(&response, &self.selection)?;
                self.terminal = true;
                Ok(Some(completion))
            }
            Some("response.failed" | "response.incomplete" | "error") => {
                let detail = if event["type"] == "response.incomplete" {
                    "ChatGPT could not finish this response. No incomplete output was accepted."
                        .into()
                } else {
                    provider_failure(0,&serde_json::to_vec(&json!({"error":event.get("error").cloned().unwrap_or_else(||event["response"]["error"].clone())})).map_err(|_|invalid("Invalid response failure"))?)
                };
                Err(invalid(&detail))
            }
            Some(_) => Ok(None),
            None => Err(invalid("Response stream event has no type")),
        }
    }
    fn finish(mut self) -> Result<ChatCompletion, ProviderError> {
        // A transport EOF is not success, even after visible text deltas. The
        // terminal event may end at EOF without its optional final blank line.
        if !self.buffer.is_empty() {
            self.buffer.extend_from_slice(b"\n\n");
            if let Some(completion) = self.drain_events()? {
                return Ok(completion);
            }
        } else if let Some(completion) = self.event()? {
            return Ok(completion);
        }
        Err(invalid("ChatGPT stream ended before response.completed"))
    }
}

fn completed_response(
    response: &Value,
    selection: &MembershipSelection,
) -> Result<ChatCompletion, ProviderError> {
    if response["status"] != "completed"
        || response.get("error").is_some_and(|error| !error.is_null())
    {
        return Err(invalid("ChatGPT did not report successful completion"));
    }
    let items = response
        .get("output")
        .and_then(|v| v.as_array())
        .ok_or_else(|| invalid("Completed response has no output items"))?;
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut call_ids = HashSet::new();
    for item in items {
        match item.get("type").and_then(|v| v.as_str()) {
            Some("message") => {
                if item["role"] != "assistant" {
                    return Err(invalid("Response contains an unexpected message role"));
                }
                for content in item
                    .get("content")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| invalid("Response message content is invalid"))?
                {
                    let value = match content["type"].as_str() {
                        Some("output_text") => content["text"].as_str(),
                        Some("refusal") => content["refusal"].as_str(),
                        _ => return Err(invalid("Response contains unsupported message content")),
                    };
                    text.push_str(value.ok_or_else(|| invalid("Response text is invalid"))?);
                }
            }
            Some("function_call") => {
                let id = item["call_id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 256)
                    .ok_or_else(|| invalid("Function call identifier is invalid"))?;
                if !call_ids.insert(id) {
                    return Err(invalid("Response contains duplicate function calls"));
                }
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 256)
                    .ok_or_else(|| invalid("Function call name is invalid"))?;
                let arguments = item["arguments"]
                    .as_str()
                    .filter(|s| s.len() <= 64 * 1024)
                    .ok_or_else(|| invalid("Function call arguments exceed their bounds"))?;
                let arguments: Value = serde_json::from_str(arguments)
                    .map_err(|_| invalid("Function call arguments are not valid JSON"))?;
                if !arguments.is_object() {
                    return Err(invalid("Function call arguments must be an object"));
                }
                calls.push(AssistantToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments,
                });
            }
            Some("reasoning") => {}
            _ => return Err(invalid("Response contains an unsupported output item")),
        }
    }
    if text.is_empty() && calls.is_empty() {
        return Err(invalid("ChatGPT completed without text or requested tools"));
    }
    let mut message =
        ChatMessage::assistant_with_tool_calls((!text.is_empty()).then_some(text), calls);
    message.provider_context = Some(ProviderTurnContext {
        provider: "chatgpt".into(),
        account_id: selection.account_id.clone(),
        model: selection.model.clone(),
        items: items.clone(),
    });
    let usage = response
        .get("usage")
        .filter(|usage| !usage.is_null())
        .map(|usage| {
            Ok::<_, ProviderError>(TokenUsage {
                prompt_tokens: usage["input_tokens"]
                    .as_u64()
                    .ok_or_else(|| invalid("Response input-token usage is invalid"))?,
                completion_tokens: usage["output_tokens"]
                    .as_u64()
                    .ok_or_else(|| invalid("Response output-token usage is invalid"))?,
                total_tokens: usage["total_tokens"]
                    .as_u64()
                    .ok_or_else(|| invalid("Response token usage is invalid"))?,
            })
        })
        .transpose()?;
    Ok(ChatCompletion {
        id: response["id"].as_str().map(str::to_string),
        model: response["model"].as_str().map(str::to_string),
        finish_reason: Some(
            if message.tool_calls.is_empty() {
                "stop"
            } else {
                "tool_calls"
            }
            .into(),
        ),
        message,
        usage,
    })
}

#[cfg(test)]
mod tests;
