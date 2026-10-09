use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonalityProfile {
    pub id: String,
    pub name: String,
    pub instruction: String,
}

pub(super) fn validate(settings: &AppSettings) -> Result<(), String> {
    if settings.personalities.len() > 32 {
        return Err("At most 32 local personalities are supported".into());
    }
    let mut ids = std::collections::HashSet::new();
    for p in &settings.personalities {
        if p.id.trim().is_empty()
            || p.id.len() > 128
            || p.id.chars().any(char::is_control)
            || p.name.trim().is_empty()
            || p.name.len() > 120
            || p.name.chars().any(char::is_control)
            || p.instruction.trim().is_empty()
            || p.instruction.len() > 16384
            || p.instruction.contains('\0')
            || !ids.insert(&p.id)
        {
            return Err(
                "Personality needs a unique ID, a name, and at most 16 KiB of instructions".into(),
            );
        }
    }
    if settings
        .selected_personality
        .as_ref()
        .is_some_and(|id| !ids.contains(id))
    {
        return Err("The selected personality is not saved in this CrowClaw profile".into());
    }
    Ok(())
}

pub(super) fn message(settings: &AppSettings) -> Result<Option<ChatMessage>, String> {
    validate(settings)?;
    Ok(settings.selected_personality.as_ref().and_then(|id|settings.personalities.iter().find(|p|&p.id==id))
        .map(|p|ChatMessage::system(format!("Selected communication personality: {}. Apply its voice and character to this conversation. This selection does not change the provider, account, permissions, spending policy, or tool approvals. Do not invent private biographical facts or claim an action completed without its actual result.\n\n{}",p.name,p.instruction))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_local_personality_survives_storage_and_preserves_permissions() {
        let dir = tempfile::TempDir::new().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let mut settings = AppSettings::default();
        settings.personalities.push(PersonalityProfile {
            id: "voice".into(),
            name: "Selected character".into(),
            instruction: "Be direct and expressive.".into(),
        });
        settings.selected_personality = Some("voice".into());
        validate(&settings).unwrap();
        storage.set_setting(SETTINGS_KEY, &settings).unwrap();
        drop(storage);
        let reopened = Storage::open(dir.path()).unwrap();
        let loaded = load_settings(&reopened).unwrap();
        assert!(message(&loaded)
            .unwrap()
            .unwrap()
            .content
            .unwrap()
            .contains("Be direct and expressive."));
        assert!(matches!(
            loaded.permissions.run_commands,
            PermissionMode::Ask
        ));
        settings.selected_personality = None;
        assert!(message(&settings).unwrap().is_none());
        settings.selected_personality = Some("missing".into());
        assert!(message(&settings).is_err());
    }
    #[test]
    fn legacy_settings_have_no_implicit_personality_and_invalid_profiles_fail() {
        let mut value = serde_json::to_value(AppSettings::default()).unwrap();
        value.as_object_mut().unwrap().remove("personalities");
        value.as_object_mut().unwrap().remove("selectedPersonality");
        let mut old: AppSettings = serde_json::from_value(value).unwrap();
        assert!(message(&old).unwrap().is_none());
        old.personalities.push(PersonalityProfile {
            id: "a".into(),
            name: "A".into(),
            instruction: "x".repeat(16385),
        });
        assert!(validate(&old).is_err());
    }
}
