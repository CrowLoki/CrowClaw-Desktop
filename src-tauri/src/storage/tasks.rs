use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::{
    json_to_value, now_ms, optional_json_to_value, optional_value_to_json, require_non_empty,
    value_to_json, MessageInput, MessageRole, Storage, StorageError, StorageResult, StoredTask,
    TaskInput, TaskStatus,
};

impl Storage {
    pub fn create_task(&self, input: &TaskInput) -> StorageResult<StoredTask> {
        validate_task(input)?;
        let now = now_ms()?;
        let payload_json = value_to_json(&input.payload)?;
        let connection = self.connection()?;
        connection.execute(
            r#"INSERT INTO tasks (
                   id, conversation_id, kind, payload_json, status,
                   cancellation_requested, created_at_ms, updated_at_ms
               ) VALUES (?1, ?2, ?3, ?4, 'queued', 0, ?5, ?5)"#,
            params![
                input.id,
                input.conversation_id,
                input.kind,
                payload_json,
                now
            ],
        )?;
        Ok(StoredTask {
            id: input.id.clone(),
            conversation_id: input.conversation_id.clone(),
            kind: input.kind.clone(),
            payload: input.payload.clone(),
            status: TaskStatus::Queued,
            cancellation_requested: false,
            result: None,
            error: None,
            created_at_ms: now,
            updated_at_ms: now,
            started_at_ms: None,
            completed_at_ms: None,
        })
    }

    pub fn get_task(&self, id: &str) -> StorageResult<Option<StoredTask>> {
        require_non_empty("task id", id)?;
        let connection = self.connection()?;
        task_from(&connection, id)
    }

    pub fn list_tasks(&self, status: Option<TaskStatus>) -> StorageResult<Vec<StoredTask>> {
        let connection = self.connection()?;
        list_tasks_from(&connection, status)
    }

    pub fn request_task_cancellation(&self, id: &str) -> StorageResult<StoredTask> {
        require_non_empty("task id", id)?;
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current =
            task_from(&transaction, id)?.ok_or_else(|| StorageError::not_found("task", id))?;
        if current.status.is_terminal() {
            return Err(StorageError::Conflict(format!(
                "task '{id}' is already {}",
                current.status.as_str()
            )));
        }
        transaction.execute(
            "UPDATE tasks SET cancellation_requested = 1, updated_at_ms = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        let task =
            task_from(&transaction, id)?.ok_or_else(|| StorageError::not_found("task", id))?;
        transaction.commit()?;
        Ok(task)
    }

    /// Applies a validated task-state transition in one transaction.
    pub fn update_task_status(
        &self,
        id: &str,
        next_status: TaskStatus,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
    ) -> StorageResult<StoredTask> {
        require_non_empty("task id", id)?;
        if next_status == TaskStatus::Succeeded && error.is_some() {
            return Err(StorageError::InvalidData(
                "a succeeded task cannot contain an error".into(),
            ));
        }
        let now = now_ms()?;
        let result_json = optional_value_to_json(result)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current =
            task_from(&transaction, id)?.ok_or_else(|| StorageError::not_found("task", id))?;
        if current.cancellation_requested
            && matches!(next_status, TaskStatus::Succeeded | TaskStatus::Failed)
        {
            return Err(StorageError::Conflict(format!(
                "task '{id}' has a cancellation request; only cancellation may finish it"
            )));
        }
        if !current.status.can_transition_to(next_status) {
            return Err(StorageError::Conflict(format!(
                "task '{id}' cannot transition from {} to {}",
                current.status.as_str(),
                next_status.as_str()
            )));
        }
        let started_at_ms = if next_status == TaskStatus::Running {
            current.started_at_ms.or(Some(now))
        } else {
            current.started_at_ms
        };
        let completed_at_ms = if next_status.is_terminal() {
            current.completed_at_ms.or(Some(now))
        } else {
            None
        };
        transaction.execute(
            r#"UPDATE tasks SET
                   status = ?2,
                   result_json = ?3,
                   error = ?4,
                   updated_at_ms = ?5,
                   started_at_ms = ?6,
                   completed_at_ms = ?7
               WHERE id = ?1"#,
            params![
                id,
                next_status.as_str(),
                result_json,
                error,
                now,
                started_at_ms,
                completed_at_ms,
            ],
        )?;
        let task =
            task_from(&transaction, id)?.ok_or_else(|| StorageError::not_found("task", id))?;
        transaction.commit()?;
        Ok(task)
    }

    /// Atomically wins terminal-state arbitration and, when supplied, appends
    /// the assistant message belonging to that terminal result.
    ///
    /// The boolean is true only for the caller that performed the transition.
    pub fn finish_task(
        &self,
        id: &str,
        next_status: TaskStatus,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
        message: Option<&MessageInput>,
    ) -> StorageResult<(StoredTask, bool)> {
        require_non_empty("task id", id)?;
        if !next_status.is_terminal() {
            return Err(StorageError::InvalidData(
                "finish_task requires a terminal status".into(),
            ));
        }
        if next_status == TaskStatus::Succeeded && error.is_some() {
            return Err(StorageError::InvalidData(
                "a succeeded task cannot contain an error".into(),
            ));
        }
        let metadata_json = message
            .map(|message| {
                require_non_empty("message id", &message.id)?;
                require_non_empty("message conversation id", &message.conversation_id)?;
                require_non_empty("message content", &message.content)?;
                if message.role != MessageRole::Assistant {
                    return Err(StorageError::InvalidData(
                        "a terminal task message must have the assistant role".into(),
                    ));
                }
                value_to_json(&message.metadata)
            })
            .transpose()?;
        let now = now_ms()?;
        let result_json = optional_value_to_json(result)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current =
            task_from(&transaction, id)?.ok_or_else(|| StorageError::not_found("task", id))?;
        if current.status == next_status {
            return Ok((current, false));
        }
        if current.cancellation_requested
            && matches!(next_status, TaskStatus::Succeeded | TaskStatus::Failed)
        {
            return Err(StorageError::Conflict(format!(
                "task '{id}' has a cancellation request; only cancellation may finish it"
            )));
        }
        if !current.status.can_transition_to(next_status) {
            return Err(StorageError::Conflict(format!(
                "task '{id}' cannot transition from {} to {}",
                current.status.as_str(),
                next_status.as_str()
            )));
        }
        if let Some(message) = message {
            if current.conversation_id.as_deref() != Some(&message.conversation_id) {
                return Err(StorageError::InvalidData(
                    "terminal task message conversation does not match the task".into(),
                ));
            }
            let ordinal: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM messages WHERE conversation_id = ?1",
                [&message.conversation_id],
                |row| row.get(0),
            )?;
            transaction.execute(
                r#"INSERT INTO messages (
                       id, conversation_id, ordinal, role, content, metadata_json, created_at_ms
                   ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
                params![
                    message.id,
                    message.conversation_id,
                    ordinal,
                    message.role.as_str(),
                    message.content,
                    metadata_json
                        .as_deref()
                        .expect("validated message metadata"),
                    now,
                ],
            )?;
            transaction.execute(
                "UPDATE conversations SET updated_at_ms = ?2 WHERE id = ?1",
                params![message.conversation_id, now],
            )?;
        }
        transaction.execute(
            r#"UPDATE tasks SET
                   status = ?2,
                   result_json = ?3,
                   error = ?4,
                   updated_at_ms = ?5,
                   completed_at_ms = ?5
               WHERE id = ?1"#,
            params![id, next_status.as_str(), result_json, error, now],
        )?;
        let task =
            task_from(&transaction, id)?.ok_or_else(|| StorageError::not_found("task", id))?;
        transaction.commit()?;
        Ok((task, true))
    }

    pub fn delete_task(&self, id: &str) -> StorageResult<bool> {
        require_non_empty("task id", id)?;
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM tasks WHERE id = ?1", [id])? > 0)
    }
}

type RawTask = (
    String,
    Option<String>,
    String,
    String,
    String,
    bool,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
);

fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawTask> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn task_from_raw(raw: RawTask) -> StorageResult<StoredTask> {
    Ok(StoredTask {
        id: raw.0,
        conversation_id: raw.1,
        kind: raw.2,
        payload: json_to_value(raw.3)?,
        status: TaskStatus::from_stored(&raw.4)?,
        cancellation_requested: raw.5,
        result: optional_json_to_value(raw.6)?,
        error: raw.7,
        created_at_ms: raw.8,
        updated_at_ms: raw.9,
        started_at_ms: raw.10,
        completed_at_ms: raw.11,
    })
}

fn task_from(connection: &Connection, id: &str) -> StorageResult<Option<StoredTask>> {
    let raw = connection
        .query_row(
            r#"SELECT id, conversation_id, kind, payload_json, status,
                      cancellation_requested, result_json, error, created_at_ms,
                      updated_at_ms, started_at_ms, completed_at_ms
               FROM tasks WHERE id = ?1"#,
            [id],
            task_row,
        )
        .optional()?;
    raw.map(task_from_raw).transpose()
}

pub(crate) fn list_tasks_from(
    connection: &Connection,
    status: Option<TaskStatus>,
) -> StorageResult<Vec<StoredTask>> {
    let columns = r#"SELECT id, conversation_id, kind, payload_json, status,
                            cancellation_requested, result_json, error, created_at_ms,
                            updated_at_ms, started_at_ms, completed_at_ms
                     FROM tasks"#;
    let raw_rows = if let Some(status) = status {
        let mut statement = connection.prepare(&format!(
            "{columns} WHERE status = ?1 ORDER BY created_at_ms DESC, id"
        ))?;
        let rows = statement.query_map([status.as_str()], task_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut statement =
            connection.prepare(&format!("{columns} ORDER BY created_at_ms DESC, id"))?;
        let rows = statement.query_map([], task_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    raw_rows.into_iter().map(task_from_raw).collect()
}

fn validate_task(input: &TaskInput) -> StorageResult<()> {
    require_non_empty("task id", &input.id)?;
    require_non_empty("task kind", &input.kind)
}
