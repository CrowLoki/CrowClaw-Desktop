use std::{mem, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::tools::{
    builtin_tool_definitions, ApprovalDecision, ApprovalStatus, ApprovalToken, ProposedAction,
    ToolExecution, ToolExecutor, ToolRequest,
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
                        tools: builtin_tool_definitions(),
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
                self.enforce_history_boundary(session)?;
                return Ok(AgentRunOutcome::Completed {
                    message: completion.message,
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
        let bytes = serde_json::to_vec(&session.messages)
            .map_err(|error| AgentError::Serialization {
                message: error.to_string(),
            })?
            .len();
        if bytes > self.limits.max_history_bytes {
            return Err(AgentError::BoundaryExceeded {
                boundary: "history_bytes".into(),
                limit: self.limits.max_history_bytes,
            });
        }
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
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::tempdir;

    use super::{AgentLimits, AgentRunOutcome, AgentRuntime, AgentSession};
    use crate::{
        agent::{
            AssistantToolCall, CancellationToken, ChatCompletion, ChatCompletionRequest,
            ChatMessage, ChatProvider, ProviderError,
        },
        tools::{ApprovalDecision, ToolExecutor, ToolPolicy},
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
            model: None,
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
        assert!(matches!(completed, AgentRunOutcome::Completed { .. }));

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

    struct FakeProvider {
        responses: Mutex<VecDeque<ChatCompletion>>,
        requests: Mutex<Vec<ChatCompletionRequest>>,
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
