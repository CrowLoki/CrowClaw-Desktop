use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Clone, Debug, Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ToolError {
    #[error("approval state is unavailable")]
    ApprovalStateUnavailable,
    #[error("approval token {token} was not found")]
    ApprovalNotFound { token: String },
    #[error("approval token {token} is still pending")]
    ApprovalPending { token: String },
    #[error("approval token {token} was already resolved as {state}")]
    ApprovalAlreadyResolved { token: String, state: String },
    #[error("approval token {token} was already consumed")]
    ApprovalAlreadyConsumed { token: String },
    #[error("no user-selected filesystem roots are configured")]
    NoAllowedRoots,
    #[error("path is outside every user-selected root: {path}")]
    PathOutsideAllowedRoots { path: String },
    #[error("invalid {tool_name} request: {message}")]
    InvalidRequest { tool_name: String, message: String },
    #[error("command execution is disabled by the current permission policy")]
    CommandsDisabled,
    #[error("{operation} failed for {path}: {message}")]
    FileSystem {
        operation: String,
        path: String,
        message: String,
    },
    #[error("text file {path} exceeds the {limit_bytes}-byte boundary")]
    FileTooLarge { path: String, limit_bytes: usize },
    #[error("file is not valid UTF-8 text: {path}")]
    NotText { path: String },
    #[error("could not start command '{program}': {message}")]
    CommandSpawn { program: String, message: String },
    #[error("command I/O failed: {message}")]
    CommandIo { message: String },
    #[error("tool operation was cancelled")]
    Cancelled,
}

impl ToolError {
    pub fn file_system(
        operation: impl Into<String>,
        path: &Path,
        error: impl std::fmt::Display,
    ) -> Self {
        Self::FileSystem {
            operation: operation.into(),
            path: path.display().to_string(),
            message: error.to_string(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::ApprovalStateUnavailable => "tool_approval_state_unavailable",
            Self::ApprovalNotFound { .. } => "tool_approval_not_found",
            Self::ApprovalPending { .. } => "tool_approval_pending",
            Self::ApprovalAlreadyResolved { .. } => "tool_approval_already_resolved",
            Self::ApprovalAlreadyConsumed { .. } => "tool_approval_already_consumed",
            Self::NoAllowedRoots => "tool_no_allowed_roots",
            Self::PathOutsideAllowedRoots { .. } => "tool_path_outside_allowed_roots",
            Self::InvalidRequest { .. } => "tool_invalid_request",
            Self::CommandsDisabled => "tool_commands_disabled",
            Self::FileSystem { .. } => "tool_filesystem",
            Self::FileTooLarge { .. } => "tool_file_too_large",
            Self::NotText { .. } => "tool_not_text",
            Self::CommandSpawn { .. } => "tool_command_spawn",
            Self::CommandIo { .. } => "tool_command_io",
            Self::Cancelled => "tool_cancelled",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::ApprovalPending { .. }
                | Self::FileSystem { .. }
                | Self::CommandSpawn { .. }
                | Self::CommandIo { .. }
        )
    }

    pub fn details(&self) -> Value {
        match self {
            Self::ApprovalNotFound { token }
            | Self::ApprovalPending { token }
            | Self::ApprovalAlreadyConsumed { token } => json!({ "token": token }),
            Self::ApprovalAlreadyResolved { token, state } => {
                json!({ "token": token, "state": state })
            }
            Self::PathOutsideAllowedRoots { path } | Self::NotText { path } => {
                json!({ "path": path })
            }
            Self::InvalidRequest { tool_name, .. } => json!({ "toolName": tool_name }),
            Self::FileSystem {
                operation, path, ..
            } => json!({ "operation": operation, "path": path }),
            Self::FileTooLarge { path, limit_bytes } => {
                json!({ "path": path, "limitBytes": limit_bytes })
            }
            Self::CommandSpawn { program, .. } => json!({ "program": program }),
            _ => Value::Null,
        }
    }
}
