use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

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
