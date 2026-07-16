use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::{
    json_to_value, now_ms, require_non_empty, value_to_json, Conversation, ConversationInput,
    Message, MessageInput, MessageRole, Storage, StorageError, StorageResult,
};

impl Storage {
    pub fn create_conversation(&self, input: &ConversationInput) -> StorageResult<Conversation> {
        validate_conversation(input)?;
        let now = now_ms()?;
        let connection = self.connection()?;
        connection.execute(
            r#"INSERT INTO conversations (
                   id, title, provider_profile_id, created_at_ms, updated_at_ms
               ) VALUES (?1, ?2, ?3, ?4, ?4)"#,
            params![input.id, input.title, input.provider_profile_id, now],
        )?;
        Ok(Conversation {
            id: input.id.clone(),
            title: input.title.clone(),
            provider_profile_id: input.provider_profile_id.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            archived_at_ms: None,
        })
    }

    pub fn get_conversation(&self, id: &str) -> StorageResult<Option<Conversation>> {
        require_non_empty("conversation id", id)?;
        let connection = self.connection()?;
        conversation_from(&connection, id)
    }

    pub fn list_conversations(&self, include_archived: bool) -> StorageResult<Vec<Conversation>> {
        let connection = self.connection()?;
        list_conversations_from(&connection, include_archived)
    }

    pub fn update_conversation(&self, input: &ConversationInput) -> StorageResult<Conversation> {
        validate_conversation(input)?;
        let now = now_ms()?;
        let connection = self.connection()?;
        let changed = connection.execute(
            r#"UPDATE conversations
               SET title = ?2, provider_profile_id = ?3, updated_at_ms = ?4
               WHERE id = ?1"#,
            params![input.id, input.title, input.provider_profile_id, now],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("conversation", &input.id));
        }
        conversation_from(&connection, &input.id)?
            .ok_or_else(|| StorageError::not_found("conversation", &input.id))
    }

    pub fn archive_conversation(&self, id: &str, archived: bool) -> StorageResult<Conversation> {
        require_non_empty("conversation id", id)?;
        let now = now_ms()?;
        let archived_at_ms = archived.then_some(now);
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE conversations SET archived_at_ms = ?2, updated_at_ms = ?3 WHERE id = ?1",
            params![id, archived_at_ms, now],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("conversation", id));
        }
        conversation_from(&connection, id)?
            .ok_or_else(|| StorageError::not_found("conversation", id))
    }

    pub fn delete_conversation(&self, id: &str) -> StorageResult<bool> {
        require_non_empty("conversation id", id)?;
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM conversations WHERE id = ?1", [id])? > 0)
    }

    /// Appends a message and assigns its per-conversation ordinal atomically.
    pub fn append_message(&self, input: &MessageInput) -> StorageResult<Message> {
        validate_message(input)?;
        let now = now_ms()?;
        let metadata_json = value_to_json(&input.metadata)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let conversation_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
            [&input.conversation_id],
            |row| row.get(0),
        )?;
        if !conversation_exists {
            return Err(StorageError::not_found(
                "conversation",
                &input.conversation_id,
            ));
        }
        let ordinal: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM messages WHERE conversation_id = ?1",
            [&input.conversation_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            r#"INSERT INTO messages (
                   id, conversation_id, ordinal, role, content, metadata_json, created_at_ms
               ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            params![
                input.id,
                input.conversation_id,
                ordinal,
                input.role.as_str(),
                input.content,
                metadata_json,
                now,
            ],
        )?;
        transaction.execute(
            "UPDATE conversations SET updated_at_ms = ?2 WHERE id = ?1",
            params![input.conversation_id, now],
        )?;
        transaction.commit()?;
        Ok(Message {
            id: input.id.clone(),
            conversation_id: input.conversation_id.clone(),
            ordinal,
            role: input.role,
            content: input.content.clone(),
            metadata: input.metadata.clone(),
            created_at_ms: now,
        })
    }

    pub fn list_messages(&self, conversation_id: &str) -> StorageResult<Vec<Message>> {
        require_non_empty("conversation id", conversation_id)?;
        let connection = self.connection()?;
        list_messages_from(&connection, conversation_id)
    }
}

type RawConversation = (String, String, Option<String>, i64, i64, Option<i64>);

fn conversation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawConversation> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn conversation_from_raw(raw: RawConversation) -> Conversation {
    Conversation {
        id: raw.0,
        title: raw.1,
        provider_profile_id: raw.2,
        created_at_ms: raw.3,
        updated_at_ms: raw.4,
        archived_at_ms: raw.5,
    }
}

fn conversation_from(connection: &Connection, id: &str) -> StorageResult<Option<Conversation>> {
    let raw = connection
        .query_row(
            r#"SELECT id, title, provider_profile_id, created_at_ms, updated_at_ms, archived_at_ms
               FROM conversations WHERE id = ?1"#,
            [id],
            conversation_row,
        )
        .optional()?;
    Ok(raw.map(conversation_from_raw))
}

pub(crate) fn list_conversations_from(
    connection: &Connection,
    include_archived: bool,
) -> StorageResult<Vec<Conversation>> {
    let sql = if include_archived {
        r#"SELECT id, title, provider_profile_id, created_at_ms, updated_at_ms, archived_at_ms
           FROM conversations ORDER BY updated_at_ms DESC, id"#
    } else {
        r#"SELECT id, title, provider_profile_id, created_at_ms, updated_at_ms, archived_at_ms
           FROM conversations WHERE archived_at_ms IS NULL ORDER BY updated_at_ms DESC, id"#
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], conversation_row)?;
    rows.map(|row| Ok(conversation_from_raw(row?))).collect()
}

pub(crate) fn list_messages_from(
    connection: &Connection,
    conversation_id: &str,
) -> StorageResult<Vec<Message>> {
    let mut statement = connection.prepare(
        r#"SELECT id, conversation_id, ordinal, role, content, metadata_json, created_at_ms
           FROM messages WHERE conversation_id = ?1 ORDER BY ordinal ASC"#,
    )?;
    let rows = statement.query_map([conversation_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;
    rows.map(|row| {
        let (id, conversation_id, ordinal, role, content, metadata, created_at_ms) = row?;
        Ok(Message {
            id,
            conversation_id,
            ordinal,
            role: MessageRole::from_stored(&role)?,
            content,
            metadata: json_to_value(metadata)?,
            created_at_ms,
        })
    })
    .collect()
}

fn validate_conversation(input: &ConversationInput) -> StorageResult<()> {
    require_non_empty("conversation id", &input.id)?;
    require_non_empty("conversation title", &input.title)
}

fn validate_message(input: &MessageInput) -> StorageResult<()> {
    require_non_empty("message id", &input.id)?;
    require_non_empty("message conversation id", &input.conversation_id)?;
    require_non_empty("message content", &input.content)
}
