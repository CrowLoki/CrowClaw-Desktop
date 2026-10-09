use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    composer::composer_from, now_ms, ConversationComposer, Storage, StorageError, StorageResult,
};

pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_ATTACHMENT_DRAFT_BYTES: u64 = 40 * 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_TEXT_ATTACHMENT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Text,
    Image,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentSummary {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub name: String,
    pub media_type: String,
    pub kind: AttachmentKind,
    pub byte_length: u64,
    pub sha256: String,
    pub created_at_ms: i64,
}

#[derive(Clone)]
pub struct AttachmentInput {
    pub id: String,
    pub name: String,
    pub media_type: String,
    pub kind: AttachmentKind,
    pub bytes: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRecord {
    pub summary: AttachmentSummary,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for AttachmentRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttachmentRecord")
            .field("summary", &self.summary)
            .finish_non_exhaustive()
    }
}

pub(super) fn validate(input: &AttachmentInput) -> StorageResult<()> {
    if input.id.trim().is_empty()
        || input.id.len() > 256
        || input.id.chars().any(char::is_control)
        || input.name.trim().is_empty()
        || input.name.len() > 255
        || matches!(input.name.as_str(), "." | "..")
        || input
            .name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        return Err(StorageError::InvalidData(
            "Invalid attachment id or filename".into(),
        ));
    }
    let valid_type = match input.kind {
        AttachmentKind::Text => matches!(
            input.media_type.as_str(),
            "text/plain" | "text/markdown" | "application/json"
        ),
        AttachmentKind::Image => matches!(
            input.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif"
        ),
        AttachmentKind::File => matches!(
            input.media_type.as_str(),
            "application/pdf"
                | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        ),
    };
    if !valid_type || input.bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(StorageError::InvalidData(
            "Unsupported attachment type or size".into(),
        ));
    }
    if input.kind == AttachmentKind::Text
        && (input.bytes.len() > MAX_TEXT_ATTACHMENT_BYTES
            || input.bytes.contains(&0)
            || std::str::from_utf8(&input.bytes).is_err())
    {
        return Err(StorageError::InvalidData(
            "Text attachment must be bounded UTF-8 without NUL".into(),
        ));
    }
    Ok(())
}

fn checked_composer(
    connection: &Connection,
    id: &str,
    expected: u32,
) -> StorageResult<ConversationComposer> {
    let current = composer_from(connection, id)?;
    if current.revision != expected || expected >= i32::MAX as u32 {
        return Err(StorageError::Conflict(
            "Conversation changed; refresh attachments before editing".into(),
        ));
    }
    Ok(current)
}

fn advance(connection: &Connection, id: &str, revision: u32) -> StorageResult<()> {
    connection.execute("INSERT INTO conversation_composers(conversation_id,revision,draft,selection_json,updated_at_ms)
        VALUES(?1,?2,'',NULL,?3) ON CONFLICT(conversation_id) DO UPDATE SET revision=excluded.revision,updated_at_ms=excluded.updated_at_ms",
        params![id,revision+1,now_ms()?])?;
    Ok(())
}

impl Storage {
    pub fn add_composer_attachments(
        &self,
        conversation_id: &str,
        expected_revision: u32,
        inputs: &[AttachmentInput],
    ) -> StorageResult<ConversationComposer> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = checked_composer(&transaction, conversation_id, expected_revision)?;
        if inputs.is_empty() || current.attachments.len() + inputs.len() > MAX_ATTACHMENTS {
            return Err(StorageError::InvalidData(
                "Select between one and eight draft attachments".into(),
            ));
        }
        let mut total: u64 = current.attachments.iter().map(|a| a.byte_length).sum();
        for input in inputs {
            validate(input)?;
            total += input.bytes.len() as u64;
            if total > MAX_ATTACHMENT_DRAFT_BYTES {
                return Err(StorageError::InvalidData(
                    "Draft attachments exceed 40 MiB".into(),
                ));
            }
            let kind = match input.kind {
                AttachmentKind::Text => "text",
                AttachmentKind::Image => "image",
                AttachmentKind::File => "file",
            };
            transaction.execute("INSERT INTO conversation_attachments(conversation_id,id,name,media_type,kind,byte_length,sha256,created_at_ms,bytes)
                VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![conversation_id,input.id,input.name,input.media_type,kind,input.bytes.len() as i64,format!("{:x}",Sha256::digest(&input.bytes)),now_ms()?,input.bytes])?;
        }
        advance(&transaction, conversation_id, expected_revision)?;
        let next = composer_from(&transaction, conversation_id)?;
        transaction.commit()?;
        Ok(next)
    }

    pub fn remove_composer_attachment(
        &self,
        conversation_id: &str,
        expected_revision: u32,
        attachment_id: &str,
    ) -> StorageResult<ConversationComposer> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        checked_composer(&transaction, conversation_id, expected_revision)?;
        if transaction.execute("DELETE FROM conversation_attachments WHERE conversation_id=?1 AND id=?2 AND message_id IS NULL",params![conversation_id,attachment_id])? != 1 {
            return Err(StorageError::not_found("draft attachment",attachment_id));
        }
        advance(&transaction, conversation_id, expected_revision)?;
        let next = composer_from(&transaction, conversation_id)?;
        transaction.commit()?;
        Ok(next)
    }

    pub fn attachment(&self, conversation_id: &str, id: &str) -> StorageResult<AttachmentRecord> {
        let connection = self.connection()?;
        let mut statement =
            connection.prepare(&format!("{SELECT} WHERE conversation_id=?1 AND id=?2"))?;
        statement
            .query_row(params![conversation_id, id], record_row)
            .optional()?
            .ok_or_else(|| StorageError::not_found("attachment", id))
    }

    pub fn message_attachments(
        &self,
        conversation_id: &str,
        message_id: &str,
    ) -> StorageResult<Vec<AttachmentRecord>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "{SELECT} WHERE conversation_id=?1 AND message_id=?2 ORDER BY created_at_ms,id"
        ))?;
        let rows = statement.query_map(params![conversation_id, message_id], record_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

const SELECT: &str = "SELECT id,conversation_id,message_id,name,media_type,kind,byte_length,sha256,created_at_ms,bytes FROM conversation_attachments";

fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentSummary> {
    let stored_length: i64 = row.get(6)?;
    let byte_length = u64::try_from(stored_length).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            6,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let kind: String = row.get(5)?;
    let kind = match kind.as_str() {
        "text" => AttachmentKind::Text,
        "image" => AttachmentKind::Image,
        "file" => AttachmentKind::File,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(AttachmentSummary {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        message_id: row.get(2)?,
        name: row.get(3)?,
        media_type: row.get(4)?,
        kind,
        byte_length,
        sha256: row.get(7)?,
        created_at_ms: row.get(8)?,
    })
}

fn record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRecord> {
    Ok(AttachmentRecord {
        summary: summary_row(row)?,
        bytes: row.get(9)?,
    })
}

pub(super) fn draft_summaries(
    connection: &Connection,
    id: &str,
) -> StorageResult<Vec<AttachmentSummary>> {
    let mut statement = connection.prepare("SELECT id,conversation_id,message_id,name,media_type,kind,byte_length,sha256,created_at_ms FROM conversation_attachments WHERE conversation_id=?1 AND message_id IS NULL ORDER BY created_at_ms,id")?;
    let rows = statement.query_map([id], summary_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(super) fn export_from(
    connection: &Connection,
    id: &str,
) -> StorageResult<Vec<AttachmentRecord>> {
    let mut statement = connection.prepare(&format!(
        "{SELECT} WHERE conversation_id=?1 ORDER BY created_at_ms,id"
    ))?;
    let rows = statement.query_map([id], record_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
