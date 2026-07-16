use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::json;

use super::{
    json_to_value, now_ms, optional_json_to_value, require_non_empty, value_to_json,
    ActionAuditEvent, ActionStatus, ProposedAction, ProposedActionInput, Storage, StorageError,
    StorageResult,
};

impl Storage {
    pub fn create_proposed_action(
        &self,
        input: &ProposedActionInput,
    ) -> StorageResult<ProposedAction> {
        validate_action(input)?;
        let now = now_ms()?;
        let request_json = value_to_json(&input.request)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            r#"INSERT INTO proposed_actions (
                   id, conversation_id, task_id, tool_name, summary, request_json,
                   status, created_at_ms, updated_at_ms
               ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?7)"#,
            params![
                input.id,
                input.conversation_id,
                input.task_id,
                input.tool_name,
                input.summary,
                request_json,
                now,
            ],
        )?;
        insert_audit(
            &transaction,
            &input.id,
            "proposed",
            &json!({"request": input.request}),
            now,
        )?;
        let action = action_from(&transaction, &input.id)?
            .ok_or_else(|| StorageError::not_found("proposed action", &input.id))?;
        transaction.commit()?;
        Ok(action)
    }

    pub fn get_proposed_action(&self, id: &str) -> StorageResult<Option<ProposedAction>> {
        require_non_empty("proposed action id", id)?;
        let connection = self.connection()?;
        action_from(&connection, id)
    }

    pub fn list_proposed_actions(
        &self,
        conversation_id: Option<&str>,
        status: Option<ActionStatus>,
    ) -> StorageResult<Vec<ProposedAction>> {
        if let Some(conversation_id) = conversation_id {
            require_non_empty("conversation id", conversation_id)?;
        }
        let connection = self.connection()?;
        list_actions_from(&connection, conversation_id, status)
    }

    pub fn approve_action(&self, id: &str, reason: Option<&str>) -> StorageResult<ProposedAction> {
        self.decide_action(id, ActionStatus::Approved, reason)
    }

    pub fn deny_action(&self, id: &str, reason: Option<&str>) -> StorageResult<ProposedAction> {
        self.decide_action(id, ActionStatus::Denied, reason)
    }

    pub fn record_action_success(
        &self,
        id: &str,
        result: &serde_json::Value,
    ) -> StorageResult<ProposedAction> {
        self.finish_action(id, ActionStatus::Succeeded, Some(result), None)
    }

    pub fn record_action_failure(&self, id: &str, error: &str) -> StorageResult<ProposedAction> {
        require_non_empty("action error", error)?;
        self.finish_action(id, ActionStatus::Failed, None, Some(error))
    }

    pub fn list_action_audit(&self, action_id: &str) -> StorageResult<Vec<ActionAuditEvent>> {
        require_non_empty("proposed action id", action_id)?;
        let connection = self.connection()?;
        list_action_audit_from(&connection, Some(action_id))
    }

    fn decide_action(
        &self,
        id: &str,
        decision: ActionStatus,
        reason: Option<&str>,
    ) -> StorageResult<ProposedAction> {
        require_non_empty("proposed action id", id)?;
        debug_assert!(matches!(
            decision,
            ActionStatus::Approved | ActionStatus::Denied
        ));
        let now = now_ms()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = action_from(&transaction, id)?
            .ok_or_else(|| StorageError::not_found("proposed action", id))?;
        if current.status != ActionStatus::Pending {
            return Err(StorageError::Conflict(format!(
                "action '{id}' has already been {}",
                current.status.as_str()
            )));
        }
        transaction.execute(
            r#"UPDATE proposed_actions
               SET status = ?2, decision_reason = ?3, decided_at_ms = ?4, updated_at_ms = ?4
               WHERE id = ?1"#,
            params![id, decision.as_str(), reason, now],
        )?;
        insert_audit(
            &transaction,
            id,
            decision.as_str(),
            &json!({"reason": reason}),
            now,
        )?;
        let action = action_from(&transaction, id)?
            .ok_or_else(|| StorageError::not_found("proposed action", id))?;
        transaction.commit()?;
        Ok(action)
    }

    fn finish_action(
        &self,
        id: &str,
        status: ActionStatus,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
    ) -> StorageResult<ProposedAction> {
        require_non_empty("proposed action id", id)?;
        debug_assert!(matches!(
            status,
            ActionStatus::Succeeded | ActionStatus::Failed
        ));
        let now = now_ms()?;
        let result_json = result.map(value_to_json).transpose()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = action_from(&transaction, id)?
            .ok_or_else(|| StorageError::not_found("proposed action", id))?;
        if current.status != ActionStatus::Approved {
            return Err(StorageError::Conflict(format!(
                "action '{id}' must be approved before recording a result; current status is {}",
                current.status.as_str()
            )));
        }
        transaction.execute(
            r#"UPDATE proposed_actions
               SET status = ?2, result_json = ?3, error = ?4, updated_at_ms = ?5
               WHERE id = ?1"#,
            params![id, status.as_str(), result_json, error, now],
        )?;
        insert_audit(
            &transaction,
            id,
            status.as_str(),
            &json!({"result": result, "error": error}),
            now,
        )?;
        let action = action_from(&transaction, id)?
            .ok_or_else(|| StorageError::not_found("proposed action", id))?;
        transaction.commit()?;
        Ok(action)
    }
}

type RawAction = (
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

fn action_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawAction> {
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
        row.get(12)?,
    ))
}

fn action_from_raw(raw: RawAction) -> StorageResult<ProposedAction> {
    Ok(ProposedAction {
        id: raw.0,
        conversation_id: raw.1,
        task_id: raw.2,
        tool_name: raw.3,
        summary: raw.4,
        request: json_to_value(raw.5)?,
        status: ActionStatus::from_stored(&raw.6)?,
        decision_reason: raw.7,
        decided_at_ms: raw.8,
        result: optional_json_to_value(raw.9)?,
        error: raw.10,
        created_at_ms: raw.11,
        updated_at_ms: raw.12,
    })
}

fn action_from(connection: &Connection, id: &str) -> StorageResult<Option<ProposedAction>> {
    let raw = connection
        .query_row(
            r#"SELECT id, conversation_id, task_id, tool_name, summary, request_json,
                      status, decision_reason, decided_at_ms, result_json, error,
                      created_at_ms, updated_at_ms
               FROM proposed_actions WHERE id = ?1"#,
            [id],
            action_row,
        )
        .optional()?;
    raw.map(action_from_raw).transpose()
}

pub(crate) fn list_actions_from(
    connection: &Connection,
    conversation_id: Option<&str>,
    status: Option<ActionStatus>,
) -> StorageResult<Vec<ProposedAction>> {
    let columns = r#"SELECT id, conversation_id, task_id, tool_name, summary, request_json,
                            status, decision_reason, decided_at_ms, result_json, error,
                            created_at_ms, updated_at_ms
                     FROM proposed_actions"#;
    let (clause, parameters): (&str, Vec<String>) = match (conversation_id, status) {
        (Some(conversation_id), Some(status)) => (
            " WHERE conversation_id = ?1 AND status = ?2",
            vec![conversation_id.to_owned(), status.as_str().to_owned()],
        ),
        (Some(conversation_id), None) => (
            " WHERE conversation_id = ?1",
            vec![conversation_id.to_owned()],
        ),
        (None, Some(status)) => (" WHERE status = ?1", vec![status.as_str().to_owned()]),
        (None, None) => ("", Vec::new()),
    };
    let sql = format!("{columns}{clause} ORDER BY created_at_ms ASC, id");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(parameters.iter()), action_row)?;
    let raw = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    raw.into_iter().map(action_from_raw).collect()
}

pub(crate) fn list_action_audit_from(
    connection: &Connection,
    action_id: Option<&str>,
) -> StorageResult<Vec<ActionAuditEvent>> {
    let columns = r#"SELECT sequence, action_id, event_kind, detail_json, created_at_ms
                     FROM action_audit"#;
    let raw = if let Some(action_id) = action_id {
        let mut statement =
            connection.prepare(&format!("{columns} WHERE action_id = ?1 ORDER BY sequence"))?;
        let rows = statement.query_map([action_id], audit_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut statement = connection.prepare(&format!("{columns} ORDER BY sequence"))?;
        let rows = statement.query_map([], audit_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    raw.into_iter()
        .map(|(sequence, action_id, event_kind, detail, created_at_ms)| {
            Ok(ActionAuditEvent {
                sequence,
                action_id,
                event_kind,
                detail: json_to_value(detail)?,
                created_at_ms,
            })
        })
        .collect()
}

fn audit_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, String, String, String, i64)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn insert_audit(
    transaction: &Transaction<'_>,
    action_id: &str,
    event_kind: &str,
    detail: &serde_json::Value,
    created_at_ms: i64,
) -> StorageResult<()> {
    transaction.execute(
        r#"INSERT INTO action_audit (action_id, event_kind, detail_json, created_at_ms)
           VALUES (?1, ?2, ?3, ?4)"#,
        params![action_id, event_kind, value_to_json(detail)?, created_at_ms],
    )?;
    Ok(())
}

fn validate_action(input: &ProposedActionInput) -> StorageResult<()> {
    require_non_empty("proposed action id", &input.id)?;
    require_non_empty("action conversation id", &input.conversation_id)?;
    require_non_empty("action tool name", &input.tool_name)?;
    require_non_empty("action summary", &input.summary)
}
