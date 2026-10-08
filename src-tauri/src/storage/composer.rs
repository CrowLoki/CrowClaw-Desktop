use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::{
    now_ms, require_non_empty, Message, MessageInput, MessageRole, Storage, StorageError,
    StorageResult, StoredTask, TaskInput,
};

pub const MAX_DRAFT_BYTES: usize = 1024 * 1024;

/// A next-turn choice references CrowClaw-owned connection configuration, never
/// credentials. Submitted tasks must retain their own immutable copy of this value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationModelChoice {
    pub provider_profile_id: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConversationComposer {
    pub conversation_id: String,
    pub revision: u32,
    pub draft: String,
    pub selection: Option<ConversationModelChoice>,
}

impl Storage {
    /// Atomically submits a user turn against the authoritative saved selection.
    /// Provider availability/capabilities must be checked by the native caller
    /// before this operation; this snapshot contains no connection credentials.
    pub fn begin_composer_turn(
        &self,
        composer: &ConversationComposer,
        message: &MessageInput,
        task: &TaskInput,
    ) -> StorageResult<(Message, StoredTask, ConversationComposer)> {
        require_non_empty("conversation id", &composer.conversation_id)?;
        require_non_empty("message id", &message.id)?;
        require_non_empty("message content", &message.content)?;
        require_non_empty("task id", &task.id)?;
        if message.conversation_id != composer.conversation_id
            || task.conversation_id.as_deref() != Some(composer.conversation_id.as_str())
            || message.role != MessageRole::User
            || task.kind != "agent-turn"
        {
            return Err(StorageError::InvalidData(
                "Composer submission requires a user message and agent-turn task in the same conversation".into(),
            ));
        }
        if message.content.len() > MAX_DRAFT_BYTES || message.content.contains('\0') {
            return Err(StorageError::InvalidData(
                "Message exceeds its text bounds".into(),
            ));
        }
        let mut message = message.clone();
        let mut task = task.clone();
        let mut metadata = submission_object(&message.metadata, "message metadata")?;
        let mut payload = submission_object(&task.payload, "task payload")?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = composer_from(&transaction, &composer.conversation_id)?;
        if current.revision != composer.revision
            || current.selection != composer.selection
            || current.revision >= i32::MAX as u32
        {
            return Err(StorageError::Conflict(
                "This conversation changed; refresh its draft and model choice before sending"
                    .into(),
            ));
        }
        let selection = current.selection.as_ref().ok_or_else(|| {
            StorageError::InvalidData("Choose a conversation model before sending".into())
        })?;
        let snapshot = serde_json::to_value(selection)?;
        metadata.insert("modelSelection".into(), snapshot.clone());
        payload.insert("modelSelection".into(), snapshot);
        payload.insert("composerRevision".into(), current.revision.into());
        message.metadata = metadata.into();
        task.payload = payload.into();
        let now = now_ms()?;
        let has_user_message: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE conversation_id=?1 AND role='user')",
            [&current.conversation_id],
            |row| row.get(0),
        )?;
        if !has_user_message {
            if let Some(title) = task
                .payload
                .get("title")
                .and_then(serde_json::Value::as_str)
                .filter(|title| !title.trim().is_empty() && title.len() <= 256)
            {
                transaction.execute(
                    "UPDATE conversations SET title=?2 WHERE id=?1",
                    params![current.conversation_id, title],
                )?;
            }
        }
        let saved_message = super::conversations::append_message_in(
            &transaction,
            &message,
            &serde_json::to_string(&message.metadata)?,
            now,
        )?;
        let saved_task = super::tasks::insert_task_in(
            &transaction,
            &task,
            &serde_json::to_string(&task.payload)?,
            now,
        )?;
        transaction.execute(
            "UPDATE conversation_composers SET draft='', revision=?2, updated_at_ms=?3 WHERE conversation_id=?1",
            params![current.conversation_id, current.revision + 1, now],
        )?;
        let next = composer_from(&transaction, &current.conversation_id)?;
        transaction.commit()?;
        Ok((saved_message, saved_task, next))
    }

    /// An untouched composer has revision zero. Reading it does not change the
    /// conversation's recency or seed it from a mutable global model default.
    pub fn conversation_composer(&self, id: &str) -> StorageResult<ConversationComposer> {
        require_non_empty("conversation id", id)?;
        let connection = self.connection()?;
        composer_from(&connection, id)
    }

    /// Revision checking prevents a delayed autosave or another window from
    /// overwriting a newer draft/model choice. The native integration must validate
    /// current provider/account capabilities before choosing, and again on send.
    pub fn save_conversation_composer(
        &self,
        id: &str,
        expected_revision: u32,
        draft: &str,
        selection: Option<&ConversationModelChoice>,
    ) -> StorageResult<ConversationComposer> {
        require_non_empty("conversation id", id)?;
        if draft.len() > MAX_DRAFT_BYTES || draft.contains('\0') {
            return Err(StorageError::InvalidData(
                "Draft exceeds its text bounds".into(),
            ));
        }
        if let Some(choice) = selection {
            for (value, max) in [(&choice.provider_profile_id, 256), (&choice.model, 256)] {
                if value.trim().is_empty() || value.len() > max || value.contains('\0') {
                    return Err(StorageError::InvalidData(
                        "Conversation model choice is invalid".into(),
                    ));
                }
            }
            if choice.reasoning_effort.as_ref().is_some_and(|effort| {
                effort.trim().is_empty() || effort.len() > 64 || effort.contains('\0')
            }) {
                return Err(StorageError::InvalidData(
                    "Conversation reasoning choice is invalid".into(),
                ));
            }
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = composer_from(&transaction, id)?;
        if current.revision != expected_revision || current.revision >= i32::MAX as u32 {
            return Err(StorageError::Conflict(
                "This conversation changed; refresh its draft and model choice before saving"
                    .into(),
            ));
        }
        if let Some(choice) = selection {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM provider_profiles WHERE id=?1)",
                [&choice.provider_profile_id],
                |row| row.get(0),
            )?;
            // Removing a connection must not prevent saving a draft that still
            // references that old choice. A new orphaned choice is rejected;
            // actual inference always revalidates availability in the runtime.
            if !exists && current.selection.as_ref() != Some(choice) {
                return Err(StorageError::not_found(
                    "provider profile",
                    &choice.provider_profile_id,
                ));
            }
        }
        let selection_json = selection.map(serde_json::to_string).transpose()?;
        let next = ConversationComposer {
            conversation_id: id.into(),
            revision: current.revision + 1,
            draft: draft.into(),
            selection: selection.cloned(),
        };
        transaction.execute(
            "INSERT INTO conversation_composers(conversation_id,revision,draft,selection_json,updated_at_ms)
             VALUES(?1,?2,?3,?4,?5) ON CONFLICT(conversation_id) DO UPDATE SET
             revision=excluded.revision,draft=excluded.draft,selection_json=excluded.selection_json,updated_at_ms=excluded.updated_at_ms",
            params![id,next.revision,draft,selection_json,now_ms()?],
        )?;
        transaction.commit()?;
        Ok(next)
    }
}

fn submission_object(
    value: &serde_json::Value,
    field: &str,
) -> StorageResult<serde_json::Map<String, serde_json::Value>> {
    match value {
        serde_json::Value::Null => Ok(serde_json::Map::new()),
        serde_json::Value::Object(object) => Ok(object.clone()),
        _ => Err(StorageError::InvalidData(format!(
            "{field} must be an object or null"
        ))),
    }
}

pub(super) fn composer_from(
    connection: &Connection,
    id: &str,
) -> StorageResult<ConversationComposer> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1)",
        [id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(StorageError::not_found("conversation", id));
    }
    let row: Option<(u32,String,Option<String>)> = connection.query_row(
        "SELECT revision,draft,selection_json FROM conversation_composers WHERE conversation_id=?1", [id],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).optional()?;
    let (revision, draft, selection) = row.unwrap_or((0, String::new(), None));
    Ok(ConversationComposer {
        conversation_id: id.into(),
        revision,
        draft,
        selection: selection
            .map(|json| serde_json::from_str(&json))
            .transpose()?,
    })
}
