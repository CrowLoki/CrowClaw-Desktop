mod cancellation;
mod config;
mod error;
mod protocol;
mod provider;
mod runtime;

pub(crate) use provider::{openai_request_body, parse_chat_completion};

pub use cancellation::CancellationToken;
pub use config::{ProviderConfig, ProviderPreset};
pub use error::{AgentError, ProviderError, StructuredError};
pub use protocol::{
    AssistantToolCall, AttachmentContent, ChatCompletion, ChatCompletionRequest, ChatMessage,
    ChatRole, ProviderTurnContext, TokenUsage, ToolDefinition,
};
pub use provider::{
    ChatProvider, OpenAiCompatibleClient, ProviderHealth, ProviderHealthState, ProviderModel,
};
pub use runtime::{AgentLimits, AgentRunOutcome, AgentRuntime, AgentSession, PendingToolCall};
