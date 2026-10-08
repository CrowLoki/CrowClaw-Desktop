use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use super::{now_ms, require_non_empty, Storage, StorageError, StorageResult};

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
