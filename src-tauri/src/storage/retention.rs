use rusqlite::{Connection, TransactionBehavior};

use super::actions::{list_action_audit_from, list_actions_from};
use super::conversations::{list_conversations_from, list_messages_from};
use super::crowquant::list_crowquant_memories_from;
use super::settings::{list_provider_profiles_from, list_settings_from};
use super::tasks::list_tasks_from;
use super::{
    now_ms, ConversationExport, RetentionChoice, RetentionReport, Storage, StorageExport,
    StorageResult, CURRENT_SCHEMA_VERSION,
};

impl Storage {
    /// Returns a transactionally consistent, JSON-serializable copy of all
    /// user-owned durable records. Secrets are not stored here; provider
    /// profiles contain only an optional external credential reference.
    pub fn export_all(&self) -> StorageResult<StorageExport> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let conversations = list_conversations_from(&transaction, true)?
            .into_iter()
            .map(|conversation| {
                let messages = list_messages_from(&transaction, &conversation.id)?;
                Ok(ConversationExport {
                    conversation,
                    messages,
                })
            })
            .collect::<StorageResult<Vec<_>>>()?;
        let export = StorageExport {
            schema_version: CURRENT_SCHEMA_VERSION,
            exported_at_ms: now_ms()?,
            settings: list_settings_from(&transaction)?,
            provider_profiles: list_provider_profiles_from(&transaction)?,
            conversations,
            tasks: list_tasks_from(&transaction, None)?,
            actions: list_actions_from(&transaction, None, None)?,
            action_audit: list_action_audit_from(&transaction, None)?,
            crowquant_memories: list_crowquant_memories_from(&transaction)?,
        };
        transaction.commit()?;
        Ok(export)
    }

    pub fn stored_record_count(&self) -> StorageResult<u64> {
        let connection = self.connection()?;
        record_count(&connection)
    }

    /// Applies the user's explicit uninstall-data choice. `Preserve` performs
    /// no writes. `Remove` clears all user records transactionally, checkpoints
    /// the WAL, and vacuums the database. The integration layer can then call
    /// `close` and remove the supplied app-data directory itself.
    pub fn apply_retention_choice(
        &self,
        choice: RetentionChoice,
    ) -> StorageResult<RetentionReport> {
        let mut connection = self.connection()?;
        let records_before = record_count(&connection)?;
        if choice == RetentionChoice::Preserve {
            return Ok(RetentionReport {
                choice,
                records_before,
                records_after: records_before,
            });
        }

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(
            r#"
            DELETE FROM action_audit;
            DELETE FROM proposed_actions;
            DELETE FROM messages;
            DELETE FROM tasks;
            DELETE FROM conversations;
            DELETE FROM provider_profiles;
            DELETE FROM settings;
            DELETE FROM crowquant_memories;
            DELETE FROM sqlite_sequence WHERE name = 'action_audit';
            "#,
        )?;
        transaction.commit()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        let records_after = record_count(&connection)?;
        Ok(RetentionReport {
            choice,
            records_before,
            records_after,
        })
    }
}

fn record_count(connection: &Connection) -> StorageResult<u64> {
    let count: i64 = connection.query_row(
        r#"SELECT
             (SELECT COUNT(*) FROM settings) +
             (SELECT COUNT(*) FROM provider_profiles) +
             (SELECT COUNT(*) FROM conversations) +
             (SELECT COUNT(*) FROM messages) +
             (SELECT COUNT(*) FROM tasks) +
             (SELECT COUNT(*) FROM proposed_actions) +
             (SELECT COUNT(*) FROM action_audit) +
             (SELECT COUNT(*) FROM crowquant_memories)"#,
        [],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(count).unwrap_or(0))
}
