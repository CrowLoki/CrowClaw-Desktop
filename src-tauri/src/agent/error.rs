use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::tools::ToolError;

#[derive(Clone, Debug, Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ProviderError {
    #[error("invalid provider configuration: {message}")]
    InvalidConfiguration { message: String },
    #[error("provider request failed: {message}")]
    Transport { message: String },
    #[error("provider returned HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("provider returned invalid data: {message}")]
    InvalidResponse { message: String },
    #[error("provider response exceeded the {limit_bytes}-byte boundary")]
    ResponseTooLarge { limit_bytes: usize },
    #[error("provider does not support {capability}")]
    Unsupported { capability: String },
    #[error("provider operation was cancelled")]
    Cancelled,
}

impl ProviderError {
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport { .. }
                | Self::HttpStatus {
                    status: 408 | 429 | 500..=599,
                    ..
                }
        )
    }
}

#[derive(Clone, Debug, Error, Serialize, Deserialize, PartialEq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum AgentError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Tool(#[from] ToolError),
    #[error("agent operation was cancelled")]
    Cancelled,
    #[error("agent boundary '{boundary}' exceeded its limit of {limit}")]
    BoundaryExceeded { boundary: String, limit: usize },
    #[error("invalid model tool call '{tool_name}': {message}")]
    InvalidToolCall { tool_name: String, message: String },
    #[error("agent session is waiting for approval token {token}")]
    AwaitingApproval { token: String },
    #[error("agent session state is invalid: {message}")]
    InvalidSession { message: String },
    #[error("could not encode agent state: {message}")]
    Serialization { message: String },
}

impl AgentError {
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::Cancelled
                | Self::Provider(ProviderError::Cancelled)
                | Self::Tool(ToolError::Cancelled)
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StructuredError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

impl From<&ProviderError> for StructuredError {
    fn from(error: &ProviderError) -> Self {
        let (code, details) = match error {
            ProviderError::InvalidConfiguration { .. } => {
                ("provider_invalid_configuration", Value::Null)
            }
            ProviderError::Transport { .. } => ("provider_transport", Value::Null),
            ProviderError::HttpStatus { status, .. } => {
                ("provider_http_status", json!({ "status": status }))
            }
            ProviderError::InvalidResponse { .. } => ("provider_invalid_response", Value::Null),
            ProviderError::ResponseTooLarge { limit_bytes } => (
                "provider_response_too_large",
                json!({ "limitBytes": limit_bytes }),
            ),
            ProviderError::Unsupported { capability } => {
                ("provider_unsupported", json!({ "capability": capability }))
            }
            ProviderError::Cancelled => ("provider_cancelled", Value::Null),
        };

        Self {
            code: code.to_string(),
            message: error.to_string(),
            retryable: error.retryable(),
            details,
        }
    }
}

impl From<&AgentError> for StructuredError {
    fn from(error: &AgentError) -> Self {
        match error {
            AgentError::Provider(error) => error.into(),
            AgentError::Tool(error) => Self {
                code: error.code().into(),
                message: error.to_string(),
                retryable: error.retryable(),
                details: error.details(),
            },
            AgentError::Cancelled => Self {
                code: "agent_cancelled".into(),
                message: error.to_string(),
                retryable: false,
                details: Value::Null,
            },
            AgentError::BoundaryExceeded { boundary, limit } => Self {
                code: "agent_boundary_exceeded".into(),
                message: error.to_string(),
                retryable: false,
                details: json!({ "boundary": boundary, "limit": limit }),
            },
            AgentError::InvalidToolCall { tool_name, .. } => Self {
                code: "agent_invalid_tool_call".into(),
                message: error.to_string(),
                retryable: false,
                details: json!({ "toolName": tool_name }),
            },
            AgentError::AwaitingApproval { token } => Self {
                code: "agent_awaiting_approval".into(),
                message: error.to_string(),
                retryable: true,
                details: json!({ "token": token }),
            },
            AgentError::InvalidSession { .. } => Self {
                code: "agent_invalid_session".into(),
                message: error.to_string(),
                retryable: false,
                details: Value::Null,
            },
            AgentError::Serialization { .. } => Self {
                code: "agent_serialization".into(),
                message: error.to_string(),
                retryable: false,
                details: Value::Null,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentError, ProviderError};
    use crate::tools::ToolError;

    #[test]
    fn cancellation_classifier_covers_runtime_provider_and_tool_boundaries() {
        assert!(AgentError::Cancelled.is_cancelled());
        assert!(AgentError::Provider(ProviderError::Cancelled).is_cancelled());
        assert!(AgentError::Tool(ToolError::Cancelled).is_cancelled());
        assert!(!AgentError::InvalidSession {
            message: "not cancellation".into()
        }
        .is_cancelled());
    }
}
