use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub(crate) fn snapshot(storage: &crate::storage::Storage) -> Result<serde_json::Value, String> {
    let export = storage.export_all().map_err(|error| error.to_string())?;
    let settings = export
        .settings
        .into_iter()
        .find(|record| record.key == "memory_settings")
        .map(|record| record.value);
    Ok(serde_json::json!({
        "schemaVersion":export.schema_version,"exportedAtMs":export.exported_at_ms,
        "memorySettings":settings,"conversations":export.conversations,
        "crowquantMemories":export.crowquant_memories,"actions":export.actions,
        "actionAudit":export.action_audit,"tasks":export.tasks,
        "sources":export.memory_sources,"chunks":export.memory_chunks,
        "exclusions":export.memory_exclusions,"embeddingProfiles":export.memory_embedding_profiles,
        "vectors":export.memory_vectors
    }))
}

/// Stage the complete JSON beside its destination and publish it by rename.
/// A failed write cannot truncate an existing export.
pub(crate) fn save_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Export destination has no parent directory")?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "Cannot serialize memory export")?;
    let temporary = parent.join(format!(".crowclaw-export-{}.tmp", uuid::Uuid::new_v4()));
    let operation = (|| -> std::io::Result<()> {
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        drop(output);
        fs::rename(&temporary, path)
    })();
    if operation.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    operation.map_err(|error| format!("Could not save memory export: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_keeps_withdrawn_notes_unindexed_conversations_and_memory_settings() {
        use crate::{
            memory::{MemoryService, MemorySettings},
            storage::{ConversationInput, MessageInput, MessageRole, Storage},
        };
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(Storage::open(dir.path()).unwrap());
        let service = MemoryService::new(storage.clone());
        service
            .configure(MemorySettings {
                index_conversations: Some(false),
                ..MemorySettings::default()
            })
            .unwrap();
        storage
            .set_setting("unrelated_private_setting", &"not-memory-export-content")
            .unwrap();
        let note = service.remember("retained note after withdrawal").unwrap();
        let source = storage.memory_sources().unwrap().remove(0);
        service.withdraw(&source.id).unwrap();
        storage
            .create_conversation(&ConversationInput {
                id: "conversation".into(),
                title: "Unindexed".into(),
                provider_profile_id: None,
            })
            .unwrap();
        storage
            .append_message(&MessageInput {
                id: "message".into(),
                conversation_id: "conversation".into(),
                role: MessageRole::User,
                content: "unindexed conversation original".into(),
                metadata: serde_json::Value::Null,
            })
            .unwrap();
        let exported = snapshot(&storage).unwrap();
        assert_eq!(exported["crowquantMemories"][0]["id"], note.id);
        assert_eq!(
            exported["crowquantMemories"][0]["text"],
            "retained note after withdrawal"
        );
        assert_eq!(
            exported["conversations"][0]["messages"][0]["content"],
            "unindexed conversation original"
        );
        assert_eq!(exported["memorySettings"]["indexConversations"], false);
        assert!(!exported.to_string().contains("not-memory-export-content"));
        assert!(exported["chunks"].as_array().unwrap().is_empty());
    }

    #[test]
    fn saves_complete_json_and_preserves_existing_file_on_failed_destination() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("memory.json");
        save_json(&target, &serde_json::json!({"source":"first"})).unwrap();
        save_json(&target, &serde_json::json!({"source":"second"})).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&target).unwrap()).unwrap()
                ["source"],
            "second"
        );
        let old = fs::read(&target).unwrap();
        assert!(save_json(&target.join("invalid.json"), &serde_json::json!({})).is_err());
        assert_eq!(fs::read(&target).unwrap(), old);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
