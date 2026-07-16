use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::ToolError;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ApprovalToken(Uuid);

impl ApprovalToken {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for ApprovalToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ActionId(Uuid);

impl ActionId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolRequest {
    ListDirectory {
        path: PathBuf,
    },
    ReadTextFile {
        path: PathBuf,
    },
    RunCommand {
        program: String,
        #[serde(default)]
        args: Vec<String>,
        cwd: PathBuf,
    },
    RememberMemory {
        text: String,
    },
    SearchMemory {
        query: String,
        #[serde(default = "default_memory_search_limit")]
        limit: usize,
    },
}

pub const MEMORY_TEXT_MAX_BYTES: usize = 16 * 1024;

fn default_memory_search_limit() -> usize {
    5
}

impl ToolRequest {
    pub fn tool_name(&self) -> &'static str {
        match self {
            Self::ListDirectory { .. } => "list_directory",
            Self::ReadTextFile { .. } => "read_text_file",
            Self::RunCommand { .. } => "run_command",
            Self::RememberMemory { .. } => "remember_memory",
            Self::SearchMemory { .. } => "search_memory",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::ListDirectory { path } => format!("List directory {}", path.display()),
            Self::ReadTextFile { path } => format!("Read text file {}", path.display()),
            Self::RunCommand { program, args, cwd } => {
                let rendered_args = args
                    .iter()
                    .map(|argument| format!("{argument:?}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    "Run command {program:?} {rendered_args} in {}",
                    cwd.display()
                )
            }
            Self::RememberMemory { text } => {
                format!("Store this text in local CrowQuant memory: {text:?}")
            }
            Self::SearchMemory { query, limit } => format!(
                "Search local CrowQuant memory for {query:?} and return up to {limit} matches"
            ),
        }
    }

    pub fn from_model_call(tool_name: &str, arguments: Value) -> Result<Self, ToolError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PathArguments {
            path: PathBuf,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct CommandArguments {
            program: String,
            #[serde(default)]
            args: Vec<String>,
            cwd: PathBuf,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RememberMemoryArguments {
            text: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct SearchMemoryArguments {
            query: String,
            #[serde(default = "default_memory_search_limit")]
            limit: usize,
        }

        let invalid = |error: serde_json::Error| ToolError::InvalidRequest {
            tool_name: tool_name.to_string(),
            message: error.to_string(),
        };

        match tool_name {
            "list_directory" => {
                let parsed: PathArguments = serde_json::from_value(arguments).map_err(invalid)?;
                Ok(Self::ListDirectory { path: parsed.path })
            }
            "read_text_file" => {
                let parsed: PathArguments = serde_json::from_value(arguments).map_err(invalid)?;
                Ok(Self::ReadTextFile { path: parsed.path })
            }
            "run_command" => {
                let parsed: CommandArguments =
                    serde_json::from_value(arguments).map_err(invalid)?;
                if parsed.program.trim().is_empty() {
                    return Err(ToolError::InvalidRequest {
                        tool_name: tool_name.into(),
                        message: "program cannot be empty".into(),
                    });
                }
                Ok(Self::RunCommand {
                    program: parsed.program,
                    args: parsed.args,
                    cwd: parsed.cwd,
                })
            }
            "remember_memory" => {
                let parsed: RememberMemoryArguments =
                    serde_json::from_value(arguments).map_err(invalid)?;
                let text = parsed.text.trim();
                if text.is_empty() {
                    return Err(ToolError::InvalidRequest {
                        tool_name: tool_name.into(),
                        message: "text cannot be empty".into(),
                    });
                }
                if text.len() > MEMORY_TEXT_MAX_BYTES {
                    return Err(ToolError::InvalidRequest {
                        tool_name: tool_name.into(),
                        message: format!("text cannot exceed {MEMORY_TEXT_MAX_BYTES} UTF-8 bytes"),
                    });
                }
                Ok(Self::RememberMemory { text: text.into() })
            }
            "search_memory" => {
                let parsed: SearchMemoryArguments =
                    serde_json::from_value(arguments).map_err(invalid)?;
                let query = parsed.query.trim();
                if query.is_empty() {
                    return Err(ToolError::InvalidRequest {
                        tool_name: tool_name.into(),
                        message: "query cannot be empty".into(),
                    });
                }
                if query.len() > MEMORY_TEXT_MAX_BYTES {
                    return Err(ToolError::InvalidRequest {
                        tool_name: tool_name.into(),
                        message: format!("query cannot exceed {MEMORY_TEXT_MAX_BYTES} UTF-8 bytes"),
                    });
                }
                if !(1..=20).contains(&parsed.limit) {
                    return Err(ToolError::InvalidRequest {
                        tool_name: tool_name.into(),
                        message: "limit must be between 1 and 20".into(),
                    });
                }
                Ok(Self::SearchMemory {
                    query: query.into(),
                    limit: parsed.limit,
                })
            }
            _ => Err(ToolError::InvalidRequest {
                tool_name: tool_name.into(),
                message: "unknown tool".into(),
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied { reason: String },
    Consumed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProposedAction {
    pub action_id: ActionId,
    pub approval_token: ApprovalToken,
    pub tool_name: String,
    pub summary: String,
    pub request: ToolRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryEntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: DirectoryEntryKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolOutput {
    DirectoryListing {
        path: PathBuf,
        entries: Vec<DirectoryEntry>,
        truncated: bool,
    },
    TextFile {
        path: PathBuf,
        content: String,
        bytes: usize,
    },
    Command {
        program: String,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        truncated: bool,
        timed_out: bool,
    },
    MemoryRemembered {
        memory: RememberedMemory,
    },
    MemorySearch {
        query: String,
        matches: Vec<MemorySearchMatch>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RememberedMemory {
    pub id: String,
    pub text: String,
    pub created_at_ms: i64,
    pub original_bytes: u64,
    pub compressed_bytes: u64,
    pub algorithm: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchMatch {
    pub id: String,
    pub text: String,
    pub created_at_ms: i64,
    pub score: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolExecution {
    Executed {
        action_id: ActionId,
        output: ToolOutput,
    },
    Denied {
        action_id: ActionId,
        reason: String,
    },
    Failed {
        action_id: ActionId,
        error: ToolError,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ToolRequest;

    #[test]
    fn parses_bounded_memory_calls_and_applies_search_default() {
        assert_eq!(
            ToolRequest::from_model_call("remember_memory", json!({ "text": "  exact memory  " }),)
                .unwrap(),
            ToolRequest::RememberMemory {
                text: "exact memory".into()
            }
        );
        assert_eq!(
            ToolRequest::from_model_call("search_memory", json!({ "query": "  exact query  " }),)
                .unwrap(),
            ToolRequest::SearchMemory {
                query: "exact query".into(),
                limit: 5
            }
        );
    }

    #[test]
    fn rejects_empty_or_out_of_range_memory_calls() {
        assert!(
            ToolRequest::from_model_call("remember_memory", json!({ "text": "   " }),).is_err()
        );
        assert!(ToolRequest::from_model_call(
            "search_memory",
            json!({ "query": "memory", "limit": 21 }),
        )
        .is_err());
    }
}
