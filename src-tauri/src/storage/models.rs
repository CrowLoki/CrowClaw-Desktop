use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{StorageError, StorageResult};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingRecord {
    pub key: String,
    pub value: Value,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderProfileInput {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub provider_kind: String,
    pub credential_reference: Option<String>,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderProfile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub provider_kind: String,
    pub credential_reference: Option<String>,
    pub is_default: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConversationInput {
    pub id: String,
    pub title: String,
    pub provider_profile_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub provider_profile_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub archived_at_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }

    pub(crate) fn from_stored(value: &str) -> StorageResult<Self> {
        match value {
            "system" => Ok(Self::System),
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            other => Err(StorageError::InvalidData(format!(
                "unknown message role '{other}'"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MessageInput {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub ordinal: i64,
    pub role: MessageRole,
    pub content: String,
    pub metadata: Value,
    pub created_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) fn from_stored(value: &str) -> StorageResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(StorageError::InvalidData(format!(
                "unknown task status '{other}'"
            ))),
        }
    }

    pub(crate) fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Queued, Self::Running | Self::Failed | Self::Cancelled)
                    | (
                        Self::Running,
                        Self::Succeeded | Self::Failed | Self::Cancelled
                    )
            )
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskInput {
    pub id: String,
    pub conversation_id: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredTask {
    pub id: String,
    pub conversation_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub status: TaskStatus,
    pub cancellation_requested: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Approved,
    Denied,
    Succeeded,
    Failed,
}

impl ActionStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn from_stored(value: &str) -> StorageResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(StorageError::InvalidData(format!(
                "unknown action status '{other}'"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposedActionInput {
    pub id: String,
    pub conversation_id: String,
    pub task_id: Option<String>,
    pub tool_name: String,
    pub summary: String,
    pub request: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProposedAction {
    pub id: String,
    pub conversation_id: String,
    pub task_id: Option<String>,
    pub tool_name: String,
    pub summary: String,
    pub request: Value,
    pub status: ActionStatus,
    pub decision_reason: Option<String>,
    pub decided_at_ms: Option<i64>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionAuditEvent {
    pub sequence: i64,
    pub action_id: String,
    pub event_kind: String,
    pub detail: Value,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrowQuantMemoryInput {
    pub id: String,
    pub text: String,
    pub block: Vec<u8>,
    pub format_version: u32,
    pub algorithm: String,
    pub dimension: u32,
    pub seed: u32,
    pub bits: u8,
    pub original_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrowQuantMemory {
    pub id: String,
    pub text: String,
    pub block: Vec<u8>,
    pub format_version: u32,
    pub algorithm: String,
    pub dimension: u32,
    pub seed: u32,
    pub bits: u8,
    pub original_bytes: u64,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConversationExport {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StorageExport {
    pub schema_version: u32,
    pub exported_at_ms: i64,
    pub settings: Vec<SettingRecord>,
    pub provider_profiles: Vec<ProviderProfile>,
    pub conversations: Vec<ConversationExport>,
    pub tasks: Vec<StoredTask>,
    pub actions: Vec<ProposedAction>,
    pub action_audit: Vec<ActionAuditEvent>,
    pub crowquant_memories: Vec<CrowQuantMemory>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionChoice {
    Preserve,
    Remove,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RetentionReport {
    pub choice: RetentionChoice,
    pub records_before: u64,
    pub records_after: u64,
}
