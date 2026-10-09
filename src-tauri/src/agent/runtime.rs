use std::{collections::HashSet, mem, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::tools::{
    builtin_tool_definitions, image_generation_tool_definition, ApprovalDecision, ApprovalStatus,
    ApprovalToken, GeneratedImage, ProposedAction, ToolExecutor, ToolOutput, ToolRequest,
};

use super::{
    AgentError, CancellationToken, ChatCompletionRequest, ChatMessage, ChatProvider, ChatRole,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentLimits {
    pub max_iterations: usize,
    pub max_tool_calls: usize,
    pub max_history_bytes: usize,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_iterations: 12,
            max_tool_calls: 24,
            max_history_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingToolCall {
    pub provider_tool_call_id: String,
    pub proposal: ProposedAction,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub iterations: usize,
    pub tool_calls: usize,
    #[serde(default)]
    pub pending_actions: Vec<PendingToolCall>,
    #[serde(skip)]
    pub generated_images: Vec<GeneratedImage>,
}

impl AgentSession {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Result<Self, AgentError> {
        let model = model.into();
        if model.trim().is_empty() {
            return Err(AgentError::InvalidSession {
                message: "model cannot be empty".into(),
            });
        }
        if messages.is_empty() {
            return Err(AgentError::InvalidSession {
                message: "session requires at least one message".into(),
            });
        }
        Ok(Self {
            model,
            messages,
            iterations: 0,
            tool_calls: 0,
            pending_actions: Vec::new(),
            generated_images: Vec::new(),
        })
    }

    pub fn push_user_message(&mut self, content: impl Into<String>) -> Result<(), AgentError> {
        if !self.pending_actions.is_empty() {
            return Err(AgentError::InvalidSession {
                message: "resolve or deny pending actions before adding another user message"
                    .into(),
            });
        }
        self.messages.push(ChatMessage::user(content));
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentRunOutcome {
    Completed {
        message: ChatMessage,
        #[serde(default)]
        reported_model: Option<String>,
        iterations: usize,
        tool_calls: usize,
    },
    AwaitingApproval {
        actions: Vec<ProposedAction>,
        iterations: usize,
        tool_calls: usize,
    },
}

pub struct AgentRuntime {
    provider: Arc<dyn ChatProvider>,
    tools: ToolExecutor,
    limits: AgentLimits,
}

impl AgentRuntime {
    pub fn new(
        provider: Arc<dyn ChatProvider>,
        tools: ToolExecutor,
        limits: AgentLimits,
    ) -> Result<Self, AgentError> {
        if limits.max_iterations == 0 {
            return Err(AgentError::InvalidSession {
                message: "max_iterations must be greater than zero".into(),
            });
        }
        if limits.max_tool_calls == 0 {
            return Err(AgentError::InvalidSession {
                message: "max_tool_calls must be greater than zero".into(),
            });
        }
        if limits.max_history_bytes == 0 {
            return Err(AgentError::InvalidSession {
                message: "max_history_bytes must be greater than zero".into(),
            });
        }
        Ok(Self {
            provider,
            tools,
            limits,
        })
    }

    pub fn tools(&self) -> &ToolExecutor {
        &self.tools
    }

    pub fn limits(&self) -> &AgentLimits {
        &self.limits
    }

    pub fn resolve_action(
        &self,
        session: &AgentSession,
        token: &ApprovalToken,
        decision: ApprovalDecision,
    ) -> Result<ApprovalStatus, AgentError> {
        if !session
            .pending_actions
            .iter()
            .any(|pending| &pending.proposal.approval_token == token)
        {
            return Err(AgentError::InvalidSession {
                message: format!("approval token {token} does not belong to this session"),
            });
        }
        self.tools.resolve(token, decision).map_err(Into::into)
    }

    /// Runs until a final assistant message, an approval boundary, cancellation, or a limit.
    pub async fn run_until_blocked(
        &self,
        session: &mut AgentSession,
        cancellation: &CancellationToken,
    ) -> Result<AgentRunOutcome, AgentError> {
        if cancellation.is_cancelled() {
            return Err(AgentError::Cancelled);
        }

        self.enforce_history_boundary(session)?;
        if let Some(outcome) = self.process_pending(session, cancellation).await? {
            return Ok(outcome);
        }

        loop {
            if cancellation.is_cancelled() {
                return Err(AgentError::Cancelled);
            }
            self.enforce_history_boundary(session)?;
            if session.iterations >= self.limits.max_iterations {
                return Err(AgentError::BoundaryExceeded {
                    boundary: "iterations".into(),
                    limit: self.limits.max_iterations,
                });
            }

            session.iterations += 1;
            let completion = self
                .provider
                .complete(
                    ChatCompletionRequest {
                        model: session.model.clone(),
                        messages: session.messages.clone(),
                        tools: {
                            let mut tools = builtin_tool_definitions();
                            if self.tools.image_generation_available() {
                                tools.push(image_generation_tool_definition());
                            }
                            tools
                        },
                        temperature: None,
                        max_tokens: None,
                    },
                    cancellation,
                )
                .await?;
            if completion.message.role != ChatRole::Assistant {
                return Err(AgentError::InvalidSession {
                    message: "provider returned a non-assistant completion".into(),
                });
            }

            let tool_calls = completion.message.tool_calls.clone();
            if tool_calls.is_empty() {
                if completion.message.content.is_none() {
                    return Err(AgentError::InvalidSession {
                        message: "provider returned neither text nor tool calls".into(),
                    });
                }
                session.messages.push(completion.message.clone());
                // A validated final answer must survive even if retaining it
                // exceeds the next-request history budget. The entry/loop
                // checks still reject any subsequent request before inference.
                return Ok(AgentRunOutcome::Completed {
                    message: completion.message,
                    reported_model: completion.model,
                    iterations: session.iterations,
                    tool_calls: session.tool_calls,
                });
            }

            if session.tool_calls.saturating_add(tool_calls.len()) > self.limits.max_tool_calls {
                return Err(AgentError::BoundaryExceeded {
                    boundary: "tool_calls".into(),
                    limit: self.limits.max_tool_calls,
                });
            }

            let mut provider_call_ids = HashSet::with_capacity(tool_calls.len());
            for call in &tool_calls {
                if call.id.trim().is_empty() {
                    return Err(AgentError::InvalidToolCall {
                        tool_name: call.name.clone(),
                        message: "provider tool-call ID cannot be empty".into(),
                    });
                }
                if !provider_call_ids.insert(call.id.clone()) {
                    return Err(AgentError::InvalidToolCall {
                        tool_name: call.name.clone(),
                        message: format!("provider returned duplicate tool-call ID {:?}", call.id),
                    });
                }
            }

            // Parse every call before recording any proposal so malformed batches are atomic.
            let requests = tool_calls
                .iter()
                .map(|call| {
                    ToolRequest::from_model_call(&call.name, call.arguments.clone()).map_err(
                        |error| AgentError::InvalidToolCall {
                            tool_name: call.name.clone(),
                            message: error.to_string(),
                        },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut pending = Vec::with_capacity(requests.len());
            for (call, request) in tool_calls.iter().zip(requests) {
                let proposal = self.tools.propose(request)?;
                pending.push(PendingToolCall {
                    provider_tool_call_id: call.id.clone(),
                    proposal,
                });
            }

            session.tool_calls += pending.len();
            session.messages.push(completion.message);
            session.pending_actions = pending;
            self.enforce_history_boundary(session)?;
            return Ok(awaiting_outcome(session));
        }
    }

    async fn process_pending(
        &self,
        session: &mut AgentSession,
        cancellation: &CancellationToken,
    ) -> Result<Option<AgentRunOutcome>, AgentError> {
        if session.pending_actions.is_empty() {
            return Ok(None);
        }

        for pending in &session.pending_actions {
            match self.tools.status(&pending.proposal.approval_token)? {
                ApprovalStatus::Pending => return Ok(Some(awaiting_outcome(session))),
                ApprovalStatus::Approved | ApprovalStatus::Denied { .. } => {}
                ApprovalStatus::Consumed => {
                    return Err(AgentError::InvalidSession {
                        message: format!(
                            "pending approval token {} was already consumed",
                            pending.proposal.approval_token
                        ),
                    })
                }
            }
        }

        let pending_actions = mem::take(&mut session.pending_actions);
        for pending in pending_actions {
            let execution = self
                .tools
                .execute(&pending.proposal.approval_token, cancellation)
                .await?;
            if let crate::tools::ToolExecution::Executed {
                output: ToolOutput::GeneratedImage { image },
                ..
            } = &execution
            {
                session.generated_images.push(image.clone());
            }
            let content =
                serde_json::to_string(&execution).map_err(|error| AgentError::Serialization {
                    message: error.to_string(),
                })?;
            session.messages.push(ChatMessage::tool(
                pending.provider_tool_call_id,
                pending.proposal.tool_name,
                content,
            ));
        }
        self.enforce_history_boundary(session)?;
        if cancellation.is_cancelled() {
            return Err(AgentError::Cancelled);
        }
        Ok(None)
    }

    fn enforce_history_boundary(&self, session: &AgentSession) -> Result<(), AgentError> {
        super::protocol::validate_message_attachments(&session.messages)?;
        let mut counter = HistoryByteCount::default();
        serde_json::to_writer(&mut counter, &HistoryProjection(&session.messages)).map_err(
            |error| AgentError::Serialization {
                message: error.to_string(),
            },
        )?;
        if counter.0 > self.limits.max_history_bytes {
            return Err(AgentError::BoundaryExceeded {
                boundary: "history_bytes".into(),
                limit: self.limits.max_history_bytes,
            });
        }
        Ok(())
    }
}

/// Count the original message shape, retaining attachment metadata but not their
/// separately bounded binary/base64 payloads. Attached text stays in the text
/// history budget. All large fields remain borrowed.
struct HistoryProjection<'a>(&'a [ChatMessage]);
impl Serialize for HistoryProjection<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for message in self.0 {
            sequence.serialize_element(&HistoryMessage(message))?;
        }
        sequence.end()
    }
}

struct HistoryMessage<'a>(&'a ChatMessage);
impl Serialize for HistoryMessage<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use super::protocol::AttachmentContent;
        use serde::ser::SerializeMap;
        #[derive(Serialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Metadata<'a> {
            Text { name: &'a str, text: &'a str },
            Image { name: &'a str, media_type: &'a str },
            File { name: &'a str, media_type: &'a str },
        }
        // Exhaustively destructure so new message fields require a budget decision.
        let ChatMessage {
            role,
            content,
            attachments,
            tool_calls,
            tool_call_id,
            name,
            provider_context,
        } = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("role", role)?;
        if let Some(content) = content {
            map.serialize_entry("content", content)?;
        }
        if !attachments.is_empty() {
            let metadata: Vec<_> = attachments
                .iter()
                .map(|part| match part {
                    AttachmentContent::Text { name, text } => Metadata::Text { name, text },
                    AttachmentContent::Image {
                        name, media_type, ..
                    } => Metadata::Image { name, media_type },
                    AttachmentContent::File {
                        name, media_type, ..
                    } => Metadata::File { name, media_type },
                })
                .collect();
            map.serialize_entry("attachments", &metadata)?;
        }
        if !tool_calls.is_empty() {
            map.serialize_entry("toolCalls", tool_calls)?;
        }
        if let Some(id) = tool_call_id {
            map.serialize_entry("toolCallId", id)?;
        }
        if let Some(name) = name {
            map.serialize_entry("name", name)?;
        }
        if let Some(context) = provider_context {
            map.serialize_entry("providerContext", context)?;
        }
        map.end()
    }
}

#[derive(Default)]
struct HistoryByteCount(usize);
impl std::io::Write for HistoryByteCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn awaiting_outcome(session: &AgentSession) -> AgentRunOutcome {
    AgentRunOutcome::AwaitingApproval {
        actions: session
            .pending_actions
            .iter()
            .map(|pending| pending.proposal.clone())
            .collect(),
        iterations: session.iterations,
        tool_calls: session.tool_calls,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        fs,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn final_answer_survives_history_overflow_but_next_request_is_rejected() {
        let answer = ChatMessage::assistant("complete answer ".repeat(64));
        let provider = Arc::new(FakeProvider::new([ChatCompletion {
            id: None,
            model: Some("reported-model".into()),
            message: answer.clone(),
            finish_reason: Some("stop".into()),
            usage: None,
        }]));
        let runtime = AgentRuntime::new(
            provider.clone(),
            ToolExecutor::new(ToolPolicy::default()).unwrap(),
            AgentLimits {
                max_history_bytes: 256,
                ..AgentLimits::default()
            },
        )
        .unwrap();
        let mut session = AgentSession::new("test", vec![ChatMessage::user("hello")]).unwrap();
        let outcome = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            AgentRunOutcome::Completed { message, reported_model, .. }
                if message == answer && reported_model.as_deref() == Some("reported-model")
        ));
        assert_eq!(session.messages.last(), Some(&answer));
        assert_eq!(provider.requests().len(), 1);

        session.push_user_message("continue").unwrap();
        let error = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            crate::agent::AgentError::BoundaryExceeded { boundary, limit: 256 }
                if boundary == "history_bytes"
        ));
        assert_eq!(provider.requests().len(), 1);
        assert_eq!(session.iterations, 1);
        assert_eq!(session.messages[1], answer);
    }

    #[tokio::test]
    async fn attachment_history_keeps_four_mib_text_and_tool_limit() {
        let provider = Arc::new(FakeProvider::new([]));
        let runtime = AgentRuntime::new(
            provider.clone(),
            ToolExecutor::new(ToolPolicy::default()).unwrap(),
            AgentLimits::default(),
        )
        .unwrap();
        for message in [
            ChatMessage::user("x".repeat(4 * 1024 * 1024 + 1)),
            ChatMessage::tool("call", "read", "x".repeat(4 * 1024 * 1024 + 1)),
        ] {
            let mut session = AgentSession::new("test", vec![message]).unwrap();
            let error = runtime
                .run_until_blocked(&mut session, &CancellationToken::new())
                .await
                .unwrap_err();
            assert!(
                matches!(error, crate::agent::AgentError::BoundaryExceeded { ref boundary, limit: 4_194_304 } if boundary == "history_bytes")
            );
        }
        assert!(provider.requests().is_empty());
    }

    #[tokio::test]
    async fn attachment_history_counts_attached_text_before_provider() {
        use crate::agent::protocol::AttachmentContent;
        let provider = Arc::new(FakeProvider::new([]));
        let runtime = AgentRuntime::new(
            provider.clone(),
            ToolExecutor::new(ToolPolicy::default()).unwrap(),
            AgentLimits::default(),
        )
        .unwrap();
        let mut user = ChatMessage::user("");
        user.attachments = (0..5)
            .map(|i| AttachmentContent::Text {
                name: format!("text-{i}.txt"),
                text: "x".repeat(1024 * 1024),
            })
            .collect();
        let mut session = AgentSession::new("test", vec![user]).unwrap();
        let error = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            matches!(error,crate::agent::AgentError::BoundaryExceeded{ref boundary,limit:4_194_304} if boundary=="history_bytes")
        );
        assert!(provider.requests().is_empty());
    }

    #[tokio::test]
    async fn attachment_history_allows_large_attachment_only_user_before_provider() {
        use crate::agent::protocol::AttachmentContent;
        use base64::{engine::general_purpose::STANDARD, Engine};
        let provider = Arc::new(FakeProvider::new([ChatCompletion {
            id: None,
            model: None,
            message: ChatMessage::assistant("accepted"),
            finish_reason: Some("stop".into()),
            usage: None,
        }]));
        let runtime = AgentRuntime::new(
            provider.clone(),
            ToolExecutor::new(ToolPolicy::default()).unwrap(),
            AgentLimits::default(),
        )
        .unwrap();
        let mut user = ChatMessage::user("");
        user.content = None;
        user.attachments.push(AttachmentContent::Image {
            name: "image.png".into(),
            media_type: "image/png".into(),
            data_base64: STANDARD.encode(vec![1; 5 * 1024 * 1024]),
        });
        let mut session = AgentSession::new("test", vec![user]).unwrap();
        assert!(matches!(
            runtime
                .run_until_blocked(&mut session, &CancellationToken::new())
                .await
                .unwrap(),
            AgentRunOutcome::Completed { .. }
        ));
        assert_eq!(provider.requests().len(), 1);
    }

    #[tokio::test]
    async fn attachment_history_validates_payload_before_provider() {
        use crate::agent::protocol::AttachmentContent;
        let provider = Arc::new(FakeProvider::new([]));
        let runtime = AgentRuntime::new(
            provider.clone(),
            ToolExecutor::new(ToolPolicy::default()).unwrap(),
            AgentLimits::default(),
        )
        .unwrap();
        for (data, count) in [("YR==", 1), ("YQ==", 9)] {
            let mut user = ChatMessage::user("");
            user.attachments = vec![
                AttachmentContent::Image {
                    name: "image.png".into(),
                    media_type: "image/png".into(),
                    data_base64: data.into()
                };
                count
            ];
            let mut session = AgentSession::new("test", vec![user]).unwrap();
            assert!(matches!(
                runtime
                    .run_until_blocked(&mut session, &CancellationToken::new())
                    .await
                    .unwrap_err(),
                crate::agent::AgentError::Provider(ProviderError::InvalidConfiguration { .. })
            ));
        }
        assert!(provider.requests().is_empty());
    }

    #[test]
    fn attachment_history_projection_preserves_metadata_and_legacy_budget() {
        use crate::agent::protocol::AttachmentContent;
        let messages = vec![
            ChatMessage::system("rules"),
            ChatMessage::user("question"),
            ChatMessage::assistant_with_tool_calls(
                None,
                vec![AssistantToolCall {
                    id: "call".into(),
                    name: "read".into(),
                    arguments: json!({"path":"file"}),
                }],
            ),
            ChatMessage::tool("call", "read", "result"),
        ];
        assert_eq!(
            serde_json::to_vec(&super::HistoryProjection(&messages)).unwrap(),
            serde_json::to_vec(&messages).unwrap()
        );
        let mut user = ChatMessage::user("question");
        user.attachments.push(AttachmentContent::Text {
            name: "note.txt".into(),
            text: "payload".into(),
        });
        let projection = serde_json::to_value(super::HistoryProjection(&[user])).unwrap();
        assert_eq!(
            projection[0]["attachments"][0],
            json!({"type":"text","name":"note.txt","text":"payload"})
        );
        let mut user = ChatMessage::user("");
        user.attachments.push(AttachmentContent::Text {
            name: "x".repeat(4 * 1024 * 1024 + 1),
            text: "payload".into(),
        });
        let runtime = AgentRuntime::new(
            Arc::new(FakeProvider::new([])),
            ToolExecutor::new(ToolPolicy::default()).unwrap(),
            AgentLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            runtime
                .enforce_history_boundary(&AgentSession::new("test", vec![user]).unwrap())
                .unwrap_err(),
            crate::agent::AgentError::BoundaryExceeded { .. }
        ));
    }

    use super::{AgentLimits, AgentRunOutcome, AgentRuntime, AgentSession};
    use crate::{
        agent::{
            AssistantToolCall, CancellationToken, ChatCompletion, ChatCompletionRequest,
            ChatMessage, ChatProvider, ProviderError,
        },
        tools::{
            ApprovalDecision, GeneratedImage, ImageGenerationBackend, MemoryBackend,
            MemorySearchMatch, RememberedMemory, ToolError, ToolExecutor, ToolPolicy,
        },
    };

    #[tokio::test]
    async fn pauses_for_approval_then_supplies_actual_tool_output_to_model() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("approved.txt");
        fs::write(&file, "the real fixture content").unwrap();
        let first = ChatCompletion {
            id: None,
            model: None,
            message: ChatMessage::assistant_with_tool_calls(
                None,
                vec![AssistantToolCall {
                    id: "call-1".into(),
                    name: "read_text_file".into(),
                    arguments: json!({ "path": file }),
                }],
            ),
            finish_reason: Some("tool_calls".into()),
            usage: None,
        };
        let second = ChatCompletion {
            id: None,
            model: Some("reported-local-fixture".into()),
            message: ChatMessage::assistant("The approved file contains real fixture content."),
            finish_reason: Some("stop".into()),
            usage: None,
        };
        let provider = Arc::new(FakeProvider::new([first, second]));
        let tools = ToolExecutor::new(ToolPolicy::for_roots([directory.path().into()])).unwrap();
        let runtime = AgentRuntime::new(provider.clone(), tools, AgentLimits::default()).unwrap();
        let mut session =
            AgentSession::new("local-model", vec![ChatMessage::user("read it")]).unwrap();

        let waiting = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        let action = match waiting {
            AgentRunOutcome::AwaitingApproval { actions, .. } => actions[0].clone(),
            other => panic!("expected approval boundary, got {other:?}"),
        };
        assert_eq!(provider.requests().len(), 1);

        let still_waiting = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(
            still_waiting,
            AgentRunOutcome::AwaitingApproval { .. }
        ));
        assert_eq!(provider.requests().len(), 1);

        runtime
            .resolve_action(&session, &action.approval_token, ApprovalDecision::Approve)
            .unwrap();
        let completed = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        assert!(
            matches!(completed, AgentRunOutcome::Completed { reported_model:Some(ref model), .. } if model=="reported-local-fixture")
        );

        let requests = provider.requests();
        let tool_message = requests[1]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("call-1"))
            .expect("second provider request should contain tool output");
        assert!(tool_message
            .content
            .as_deref()
            .unwrap()
            .contains("the real fixture content"));
    }

    struct FakeImageGenerator(std::sync::atomic::AtomicUsize);
    #[async_trait::async_trait]
    impl ImageGenerationBackend for FakeImageGenerator {
        async fn generate(
            &self,
            prompt: &str,
            quality: &str,
            size: &str,
            cancellation: &CancellationToken,
        ) -> Result<GeneratedImage, ToolError> {
            assert_eq!(prompt, "a purple crow");
            assert_eq!(quality, "medium");
            assert_eq!(size, "1536x1024");
            if cancellation.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(GeneratedImage {
                id: "image-id".into(),
                name: "crowclaw-image-test.png".into(),
                media_type: "image/png".into(),
                model: "gpt-image-2".into(),
                bytes: vec![1, 2, 3, 4],
            })
        }
    }

    #[tokio::test]
    async fn image_generation_is_offered_only_with_a_backend_and_waits_for_approval() {
        let first = ChatCompletion {
            id: None,
            model: None,
            message: ChatMessage::assistant_with_tool_calls(
                None,
                vec![AssistantToolCall {
                    id: "image-call".into(),
                    name: "generate_image".into(),
                    arguments: json!({"prompt":"a purple crow"}),
                }],
            ),
            finish_reason: Some("tool_calls".into()),
            usage: None,
        };
        let second = ChatCompletion {
            id: None,
            model: Some("gpt-6-luna".into()),
            message: ChatMessage::assistant("Image generated."),
            finish_reason: Some("stop".into()),
            usage: None,
        };
        let provider = Arc::new(FakeProvider::new([first, second]));
        let image = Arc::new(FakeImageGenerator(std::sync::atomic::AtomicUsize::new(0)));
        let tools = ToolExecutor::new(ToolPolicy::default())
            .unwrap()
            .with_image_generator(image.clone());
        let runtime = AgentRuntime::new(provider.clone(), tools, AgentLimits::default()).unwrap();
        let mut session =
            AgentSession::new("gpt-6-luna", vec![ChatMessage::user("draw a crow")]).unwrap();
        let waiting = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        let action = match waiting {
            AgentRunOutcome::AwaitingApproval { actions, .. } => actions[0].clone(),
            other => panic!("expected image approval, got {other:?}"),
        };
        assert_eq!(image.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(provider.requests()[0]
            .tools
            .iter()
            .any(|tool| tool.name == "generate_image"));
        runtime
            .resolve_action(&session, &action.approval_token, ApprovalDecision::Approve)
            .unwrap();
        assert!(matches!(
            runtime
                .run_until_blocked(&mut session, &CancellationToken::new())
                .await
                .unwrap(),
            AgentRunOutcome::Completed { .. }
        ));
        assert_eq!(image.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(session.generated_images[0].id, "image-id");
        let requests = provider.requests();
        let result = requests[1]
            .messages
            .iter()
            .find(|m| m.name.as_deref() == Some("generate_image"))
            .unwrap()
            .content
            .as_deref()
            .unwrap();
        assert!(result.contains("image-id"));
        assert!(!result.contains("bytes"));
        assert!(!result.contains("AQIDBA=="));
        let local = ToolExecutor::new(ToolPolicy::default()).unwrap();
        assert!(!local.image_generation_available());
    }

    #[tokio::test]
    async fn enforces_iteration_boundary_before_calling_provider() {
        let provider = Arc::new(FakeProvider::new([]));
        let tools = ToolExecutor::new(ToolPolicy::default()).unwrap();
        let runtime = AgentRuntime::new(
            provider.clone(),
            tools,
            AgentLimits {
                max_iterations: 1,
                ..AgentLimits::default()
            },
        )
        .unwrap();
        let mut session =
            AgentSession::new("local-model", vec![ChatMessage::user("hello")]).unwrap();
        session.iterations = 1;

        let error = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            crate::agent::AgentError::BoundaryExceeded { .. }
        ));
        assert!(provider.requests().is_empty());
    }

    #[tokio::test]
    async fn two_memory_calls_wait_for_both_approvals_and_keep_results_bound_to_call_ids() {
        let first = ChatCompletion {
            id: None,
            model: None,
            message: ChatMessage::assistant_with_tool_calls(
                None,
                vec![
                    AssistantToolCall {
                        id: "call-remember".into(),
                        name: "remember_memory".into(),
                        arguments: json!({ "text": "qubit calibration" }),
                    },
                    AssistantToolCall {
                        id: "call-search".into(),
                        name: "search_memory".into(),
                        arguments: json!({ "query": "qubit", "limit": 3 }),
                    },
                ],
            ),
            finish_reason: Some("tool_calls".into()),
            usage: None,
        };
        let second = ChatCompletion {
            id: None,
            model: None,
            message: ChatMessage::assistant("Memory actions completed."),
            finish_reason: Some("stop".into()),
            usage: None,
        };
        let provider = Arc::new(FakeProvider::new([first, second]));
        let memory = Arc::new(RuntimeMemoryBackend::default());
        let tools = ToolExecutor::new(ToolPolicy::default())
            .unwrap()
            .with_memory_backend(memory.clone());
        let runtime = AgentRuntime::new(provider.clone(), tools, AgentLimits::default()).unwrap();
        let mut session = AgentSession::new(
            "local-model",
            vec![ChatMessage::user("remember then find it")],
        )
        .unwrap();

        let waiting = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        let actions = match waiting {
            AgentRunOutcome::AwaitingApproval { actions, .. } => actions,
            other => panic!("expected approval boundary, got {other:?}"),
        };
        assert_eq!(actions.len(), 2);
        assert_eq!(memory.remember_calls.load(Ordering::SeqCst), 0);
        assert_eq!(memory.search_calls.load(Ordering::SeqCst), 0);

        runtime
            .resolve_action(
                &session,
                &actions[0].approval_token,
                ApprovalDecision::Approve,
            )
            .unwrap();
        let still_waiting = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(
            still_waiting,
            AgentRunOutcome::AwaitingApproval { .. }
        ));
        assert_eq!(memory.remember_calls.load(Ordering::SeqCst), 0);
        assert_eq!(memory.search_calls.load(Ordering::SeqCst), 0);

        runtime
            .resolve_action(
                &session,
                &actions[1].approval_token,
                ApprovalDecision::Approve,
            )
            .unwrap();
        let completed = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(completed, AgentRunOutcome::Completed { .. }));
        assert_eq!(memory.remember_calls.load(Ordering::SeqCst), 1);
        assert_eq!(memory.search_calls.load(Ordering::SeqCst), 1);

        let requests = provider.requests();
        let remember_output = requests[1]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("call-remember"))
            .and_then(|message| message.content.as_deref())
            .expect("remember output should be supplied to the model");
        assert!(remember_output.contains("memory_remembered"));
        assert!(remember_output.contains("memory-remember"));
        let search_output = requests[1]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some("call-search"))
            .and_then(|message| message.content.as_deref())
            .expect("search output should be supplied to the model");
        assert!(search_output.contains("memory_search"));
        assert!(search_output.contains("search-result"));
    }

    #[tokio::test]
    async fn duplicate_provider_tool_call_ids_are_rejected_before_proposal_or_memory_access() {
        let completion = ChatCompletion {
            id: None,
            model: None,
            message: ChatMessage::assistant_with_tool_calls(
                None,
                vec![
                    AssistantToolCall {
                        id: "duplicate-call".into(),
                        name: "remember_memory".into(),
                        arguments: json!({ "text": "first" }),
                    },
                    AssistantToolCall {
                        id: "duplicate-call".into(),
                        name: "search_memory".into(),
                        arguments: json!({ "query": "first" }),
                    },
                ],
            ),
            finish_reason: Some("tool_calls".into()),
            usage: None,
        };
        let provider = Arc::new(FakeProvider::new([completion]));
        let memory = Arc::new(RuntimeMemoryBackend::default());
        let tools = ToolExecutor::new(ToolPolicy::default())
            .unwrap()
            .with_memory_backend(memory.clone());
        let runtime = AgentRuntime::new(provider, tools, AgentLimits::default()).unwrap();
        let mut session = AgentSession::new(
            "local-model",
            vec![ChatMessage::user("make two memory calls")],
        )
        .unwrap();

        let error = runtime
            .run_until_blocked(&mut session, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            crate::agent::AgentError::InvalidToolCall { message, .. }
                if message.contains("duplicate tool-call ID")
        ));
        assert!(session.pending_actions.is_empty());
        assert_eq!(session.tool_calls, 0);
        assert_eq!(memory.remember_calls.load(Ordering::SeqCst), 0);
        assert_eq!(memory.search_calls.load(Ordering::SeqCst), 0);
    }

    struct FakeProvider {
        responses: Mutex<VecDeque<ChatCompletion>>,
        requests: Mutex<Vec<ChatCompletionRequest>>,
    }

    #[derive(Default)]
    struct RuntimeMemoryBackend {
        remember_calls: AtomicUsize,
        search_calls: AtomicUsize,
    }

    impl MemoryBackend for RuntimeMemoryBackend {
        fn remember(
            &self,
            _action_id: &crate::tools::ActionId,
            text: &str,
            cancellation: &CancellationToken,
        ) -> Result<RememberedMemory, ToolError> {
            if cancellation.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            self.remember_calls.fetch_add(1, Ordering::SeqCst);
            Ok(RememberedMemory {
                id: "memory-remember".into(),
                text: text.into(),
                created_at_ms: 1,
                original_bytes: 2048,
                compressed_bytes: 161,
                algorithm: "CrowQuant test".into(),
            })
        }

        fn search(
            &self,
            query: &str,
            _limit: usize,
            cancellation: &CancellationToken,
        ) -> Result<Vec<MemorySearchMatch>, ToolError> {
            if cancellation.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            self.search_calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![MemorySearchMatch {
                id: "search-result".into(),
                text: format!("stored {query} memory"),
                created_at_ms: 1,
                score: 0.75,
                provenance: None,
            }])
        }
    }

    impl FakeProvider {
        fn new(responses: impl IntoIterator<Item = ChatCompletion>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<ChatCompletionRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ChatProvider for FakeProvider {
        async fn complete(
            &self,
            request: ChatCompletionRequest,
            cancellation: &CancellationToken,
        ) -> Result<ChatCompletion, ProviderError> {
            if cancellation.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }
            self.requests.lock().unwrap().push(request);
            self.responses.lock().unwrap().pop_front().ok_or_else(|| {
                ProviderError::InvalidResponse {
                    message: "fake provider ran out of responses".into(),
                }
            })
        }
    }
}
