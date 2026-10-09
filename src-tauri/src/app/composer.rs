use super::*;
use crate::storage::{composer::ConversationModelChoice, ConversationComposer};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerModel {
    id: String,
    display_name: String,
    reasoning_efforts: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerModelSource {
    billing: String,
    id: String,
    label: String,
    provider: String,
    status: String,
    models: Vec<ComposerModel>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerSnapshot {
    hidden_model_keys: Vec<String>,
    composer: ConversationComposer,
    connection: Option<ModelConnection>,
    sources: Vec<ComposerModelSource>,
    warning: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftRequest {
    conversation_id: String,
    revision: u32,
    draft: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChoiceRequest {
    conversation_id: String,
    revision: u32,
    selection: ConversationModelChoice,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRequest {
    source_id: String,
}

fn save_hidden_models(storage: &Storage, hidden: &[String]) -> Result<Vec<String>, String> {
    if hidden.len() > 4096 || hidden.iter().map(String::len).sum::<usize>() > 1024 * 1024 {
        return Err("Model visibility preferences exceed their bound".into());
    }
    let mut unique = std::collections::HashSet::new();
    for key in hidden {
        let pair: Vec<String> =
            serde_json::from_str(key).map_err(|_| "Invalid model visibility key")?;
        if pair.len() != 2
            || pair
                .iter()
                .any(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
            || !unique.insert(key)
        {
            return Err("Invalid or duplicate model visibility key".into());
        }
    }
    storage
        .set_setting("model-picker-hidden", &hidden)
        .map_err(display_error)?;
    Ok(hidden.to_vec())
}
#[tauri::command]
pub fn crowclaw_model_picker_visibility(
    state: State<'_, AppState>,
    hidden_model_keys: Vec<String>,
) -> Result<Vec<String>, String> {
    save_hidden_models(&state.storage, &hidden_model_keys)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalCatalog {
    base_url: String,
    provider_kind: String,
    models: Vec<String>,
}

fn catalog_key(id: &str) -> String {
    format!("composer-model-catalog:{id}")
}
fn local_models(storage: &Storage, profile: &ProviderProfile) -> Result<Vec<String>, String> {
    let catalog: Option<LocalCatalog> = storage
        .get_setting(&catalog_key(&profile.id))
        .map_err(display_error)?;
    let mut models = catalog
        .filter(|c| c.base_url == profile.base_url && c.provider_kind == profile.provider_kind)
        .map(|c| c.models)
        .unwrap_or_default();
    if !models.contains(&profile.model) {
        models.push(profile.model.clone());
    }
    Ok(models)
}
fn membership_source(account: &MembershipAccount) -> ComposerModelSource {
    ComposerModelSource {
        billing: "membership".into(),
        id: format!("membership:{}", account.id),
        label: account.label.clone(),
        provider: "chatgpt".into(),
        status: if account.has_credentials {
            "ready"
        } else {
            "disconnected"
        }
        .into(),
        models: account
            .catalog
            .as_ref()
            .filter(|c| account.has_credentials && c.account_id == account.id)
            .map(|c| {
                c.models
                    .iter()
                    .map(|m| ComposerModel {
                        id: m.slug.clone(),
                        display_name: m.display_name.clone(),
                        reasoning_efforts: m.reasoning_efforts.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}
fn sources(state: &AppState) -> Result<Vec<ComposerModelSource>, String> {
    let mut sources = Vec::new();
    for profile in state
        .storage
        .list_provider_profiles()
        .map_err(display_error)?
    {
        if profile.provider_kind == "chatgpt" {
            continue;
        }
        if profile.provider_kind == "openrouter" {
            let connected = state
                .storage
                .has_openrouter_key(&profile.id)
                .map_err(display_error)?;
            sources.push(ComposerModelSource {
                billing: "free".into(),
                id: profile.id.clone(),
                label: profile.name.clone(),
                provider: "openrouter".into(),
                status: if connected { "ready" } else { "disconnected" }.into(),
                models: openrouter::cached_catalog(&state.storage, &profile.id)
                    .map(|catalog| {
                        catalog
                            .models
                            .into_iter()
                            .map(|model| ComposerModel {
                                id: model.id,
                                display_name: model.name,
                                reasoning_efforts: model.reasoning_efforts,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            });
            continue;
        }
        sources.push(ComposerModelSource {
            billing: if profile.provider_kind == "crowbot-ai" {
                "free"
            } else if reqwest::Url::parse(&profile.base_url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .is_some_and(|host| {
                    host == "localhost"
                        || host
                            .trim_matches(['[', ']'])
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                })
            {
                "local"
            } else {
                "unknown"
            }
            .into(),
            id: profile.id.clone(),
            label: profile.name.clone(),
            provider: profile.provider_kind.clone(),
            status: "ready".into(),
            models: local_models(&state.storage, &profile)?
                .into_iter()
                .map(|id| ComposerModel {
                    display_name: if profile.provider_kind == "crowbot-ai" {
                        "CrowBot AI".into()
                    } else {
                        id.clone()
                    },
                    id,
                    reasoning_efforts: Vec::new(),
                })
                .collect(),
        });
    }
    sources.extend(
        state
            .storage
            .membership_accounts()
            .map_err(display_error)?
            .iter()
            .filter(|account| account.identity.provider == "chatgpt")
            .map(membership_source),
    );
    Ok(sources)
}

pub(super) fn ensure(state: &AppState, id: &str) -> Result<ConversationComposer, String> {
    let saved = state
        .storage
        .conversation_composer(id)
        .map_err(display_error)?;
    if saved.revision != 0 {
        return Ok(saved);
    }
    // Legacy chats used the global default. Snapshot it once, then the chat owns
    // its choice; later global changes cannot redirect an existing conversation.
    let default = state
        .storage
        .default_provider_profile()
        .map_err(display_error)?;
    if let Some(profile) = default
        .as_ref()
        .filter(|p| !matches!(p.provider_kind.as_str(), "chatgpt" | "openrouter"))
    {
        if state
            .storage
            .get_setting::<LocalCatalog>(&catalog_key(&profile.id))
            .map_err(display_error)?
            .is_none()
        {
            state
                .storage
                .set_setting(
                    &catalog_key(&profile.id),
                    &LocalCatalog {
                        base_url: profile.base_url.clone(),
                        provider_kind: profile.provider_kind.clone(),
                        models: vec![profile.model.clone()],
                    },
                )
                .map_err(display_error)?;
        }
    }
    let selection = default.map(|profile| {
        let effort = profile
            .credential_reference
            .as_ref()
            .and_then(|id| state.storage.membership_account(id).ok())
            .and_then(|account| account.selection)
            .filter(|s| s.model == profile.model)
            .and_then(|s| s.reasoning_effort);
        ConversationModelChoice {
            provider_profile_id: profile.id,
            model: profile.model,
            reasoning_effort: effort,
        }
    });
    match state
        .storage
        .save_conversation_composer(id, 0, &saved.draft, selection.as_ref())
    {
        Ok(saved) => Ok(saved),
        Err(StorageError::Conflict(_)) => state
            .storage
            .conversation_composer(id)
            .map_err(display_error),
        Err(error) => Err(display_error(error)),
    }
}

pub(super) fn profile_for_choice(
    state: &AppState,
    choice: &ConversationModelChoice,
) -> Result<ProviderProfile, String> {
    let mut profile = state
        .storage
        .get_provider_profile(&choice.provider_profile_id)
        .map_err(display_error)?
        .ok_or("This connection was removed; choose another connection for this chat")?;
    if profile.provider_kind == "openrouter" {
        profile.model = choice.model.clone();
        openrouter::cached_model(state, &profile, choice.reasoning_effort.as_deref())?;
        return Ok(profile);
    }
    if profile.provider_kind == "chatgpt" {
        let account_id = profile
            .credential_reference
            .as_ref()
            .ok_or("Saved account is unavailable")?;
        state
            .storage
            .membership_validate_selection(&MembershipSelection {
                account_id: account_id.clone(),
                model: choice.model.clone(),
                reasoning_effort: choice.reasoning_effort.clone(),
            })
            .map_err(display_error)?;
    } else {
        if choice.reasoning_effort.is_some() {
            return Err("This provider has not advertised reasoning levels".into());
        }
        if !local_models(&state.storage, &profile)?.contains(&choice.model) {
            return Err("Refresh models and choose an offered model for this connection".into());
        }
    }
    profile.model = choice.model.clone();
    Ok(profile)
}
pub(super) fn provider_for_choice(
    state: &AppState,
    choice: &ConversationModelChoice,
) -> Result<(ProviderProfile, Arc<dyn ChatProvider>), String> {
    let profile = profile_for_choice(state, choice)?;
    let provider: Arc<dyn ChatProvider> = if profile.provider_kind == "openrouter" {
        openrouter::provider(state, &profile, choice.reasoning_effort.clone())?
    } else if profile.provider_kind == "chatgpt" {
        Arc::new(MembershipProvider::new(
            state.memberships.clone(),
            MembershipSelection {
                account_id: profile
                    .credential_reference
                    .clone()
                    .ok_or("Saved account is unavailable")?,
                model: choice.model.clone(),
                reasoning_effort: choice.reasoning_effort.clone(),
            },
        )?)
    } else {
        provider_for_profile(state, &profile)?
    };
    Ok((profile, provider))
}
pub(super) fn snapshot(
    state: &AppState,
    saved: ConversationComposer,
) -> Result<ComposerSnapshot, String> {
    let (connection, warning) = match saved.selection.as_ref() {
        Some(choice) => match profile_for_choice(state, choice) {
            Ok(profile) => (Some(connection_view(&profile, "connected", None)), None),
            Err(error) => (None, Some(error)),
        },
        None => (
            None,
            Some("Choose a connection and model for this conversation".into()),
        ),
    };
    Ok(ComposerSnapshot {
        hidden_model_keys: state
            .storage
            .get_setting::<Vec<String>>("model-picker-hidden")
            .map_err(display_error)?
            .unwrap_or_default(),
        composer: saved,
        connection,
        sources: sources(state)?,
        warning,
    })
}

#[tauri::command]
pub fn crowclaw_composer_get(
    state: State<'_, AppState>,
    request: ConversationRequest,
) -> Result<ComposerSnapshot, String> {
    snapshot(&state, ensure(&state, &request.conversation_id)?)
}
#[tauri::command]
pub fn crowclaw_composer_save_draft(
    state: State<'_, AppState>,
    request: DraftRequest,
) -> Result<ComposerSnapshot, String> {
    let current = state
        .storage
        .conversation_composer(&request.conversation_id)
        .map_err(display_error)?;
    let saved = state
        .storage
        .save_conversation_composer(
            &request.conversation_id,
            request.revision,
            &request.draft,
            current.selection.as_ref(),
        )
        .map_err(display_error)?;
    snapshot(&state, saved)
}
#[tauri::command]
pub fn crowclaw_composer_choose(
    state: State<'_, AppState>,
    request: ChoiceRequest,
) -> Result<ComposerSnapshot, String> {
    choose(&state, request)
}
fn choose(state: &AppState, request: ChoiceRequest) -> Result<ComposerSnapshot, String> {
    let current = state
        .storage
        .conversation_composer(&request.conversation_id)
        .map_err(display_error)?;
    if current.revision != request.revision {
        return Err("This conversation changed; reload its composer before choosing".into());
    }
    if state
        .storage
        .get_provider_profile(&request.selection.provider_profile_id)
        .map_err(display_error)?
        .is_none()
    {
        let id = request
            .selection
            .provider_profile_id
            .strip_prefix("membership:")
            .ok_or("Connection was not found")?;
        let account = state
            .storage
            .membership_validate_selection(&MembershipSelection {
                account_id: id.into(),
                model: request.selection.model.clone(),
                reasoning_effort: request.selection.reasoning_effort.clone(),
            })
            .map_err(display_error)?;
        state
            .storage
            .save_provider_profile(&ProviderProfileInput {
                id: request.selection.provider_profile_id.clone(),
                name: format!("ChatGPT — {}", account.label),
                base_url: crate::membership::protocol::RESOURCE.into(),
                model: request.selection.model.clone(),
                provider_kind: "chatgpt".into(),
                credential_reference: Some(account.id),
                is_default: false,
            })
            .map_err(display_error)?;
    }
    profile_for_choice(state, &request.selection)?;
    let saved = state
        .storage
        .save_conversation_composer(
            &request.conversation_id,
            request.revision,
            &current.draft,
            Some(&request.selection),
        )
        .map_err(display_error)?;
    snapshot(state, saved)
}
#[tauri::command]
pub async fn crowclaw_composer_refresh_models(
    state: State<'_, AppState>,
    request: SourceRequest,
) -> Result<ComposerModelSource, String> {
    if let Some(id) = request.source_id.strip_prefix("membership:") {
        return state
            .memberships
            .refresh_catalog(id)
            .await
            .map(|account| membership_source(&account));
    }
    let profile = state
        .storage
        .get_provider_profile(&request.source_id)
        .map_err(display_error)?
        .ok_or("Connection was not found")?;
    if profile.provider_kind == "openrouter" {
        openrouter::refresh(&state, &profile.id).await?;
        return sources(&state)?
            .into_iter()
            .find(|source| source.id == profile.id)
            .ok_or("OpenRouter connection was not found".into());
    }
    if profile.provider_kind == "chatgpt" {
        return Err("Select the account's membership connection".into());
    }
    let models = if profile.provider_kind == "crowbot-ai" {
        crate::crowbot::CrowBotProvider::new(
            &profile.base_url,
            state.storage.crowbot_key(&profile).map_err(display_error)?,
        )
        .map_err(display_error)?
        .list_models(&CancellationToken::new())
        .await
        .map_err(display_error)?
    } else {
        OpenAiCompatibleClient::new(config_from_profile(&state, &profile)?)
            .map_err(display_error)?
            .list_models(&CancellationToken::new())
            .await
            .map_err(display_error)?
    };
    if models.len() > 256
        || models
            .iter()
            .any(|m| m.id.len() > 256 || m.id.contains('\0'))
    {
        return Err("Model catalog exceeds its bounds".into());
    }
    let current = state
        .storage
        .get_provider_profile(&profile.id)
        .map_err(display_error)?
        .ok_or("Connection was removed during refresh")?;
    if current.base_url != profile.base_url || current.provider_kind != profile.provider_kind {
        return Err("Connection changed during refresh; retry on its current configuration".into());
    }
    state
        .storage
        .set_setting(
            &catalog_key(&profile.id),
            &LocalCatalog {
                base_url: profile.base_url,
                provider_kind: profile.provider_kind,
                models: models.into_iter().map(|m| m.id).collect(),
            },
        )
        .map_err(display_error)?;
    sources(&state)?
        .into_iter()
        .find(|s| s.id == request.source_id)
        .ok_or("Connection was not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picker_visibility_persists_without_changing_models_drafts_or_default() {
        let (dir, state) = setup();
        let chat = state
            .storage
            .create_conversation(&crate::storage::ConversationInput {
                id: "visibility-chat".into(),
                title: "Visibility".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let before = ensure(&state, &chat.id).unwrap();
        let key = serde_json::to_string(&vec!["local-a", "model-a"]).unwrap();
        assert_eq!(
            save_hidden_models(&state.storage, &[key.clone()]).unwrap(),
            vec![key.clone()]
        );
        assert_eq!(ensure(&state, &chat.id).unwrap(), before);
        assert_eq!(
            state
                .storage
                .default_provider_profile()
                .unwrap()
                .unwrap()
                .id,
            "local-a"
        );
        assert!(save_hidden_models(&state.storage, &["invalid".into()]).is_err());
        assert!(save_hidden_models(&state.storage, &[key.clone(), key.clone()]).is_err());
        drop(state);
        let reopened = AppState::open(dir.path().into()).unwrap();
        assert_eq!(
            reopened
                .storage
                .get_setting::<Vec<String>>("model-picker-hidden")
                .unwrap()
                .unwrap(),
            vec![key]
        );
    }
    fn setup() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::TempDir::new().unwrap();
        let state = AppState::open(dir.path().into()).unwrap();
        state
            .storage
            .save_provider_profile(&ProviderProfileInput {
                id: "local-a".into(),
                name: "Local A".into(),
                base_url: "http://127.0.0.1:1234/v1".into(),
                model: "model-a".into(),
                provider_kind: "lm-studio".into(),
                credential_reference: None,
                is_default: true,
            })
            .unwrap();
        for id in ["chat-a", "chat-b"] {
            state
                .storage
                .create_conversation(&ConversationInput {
                    id: id.into(),
                    title: id.into(),
                    provider_profile_id: None,
                })
                .unwrap();
        }
        (dir, state)
    }
    #[test]
    fn initial_choice_is_seeded_once_and_later_defaults_do_not_redirect_it() {
        let (_dir, state) = setup();
        let a = ensure(&state, "chat-a").unwrap();
        state
            .storage
            .save_provider_profile(&ProviderProfileInput {
                id: "local-b".into(),
                name: "Local B".into(),
                base_url: "http://127.0.0.1:11434/v1".into(),
                model: "model-b".into(),
                provider_kind: "ollama".into(),
                credential_reference: None,
                is_default: true,
            })
            .unwrap();
        assert_eq!(ensure(&state, "chat-a").unwrap(), a);
        assert_eq!(
            ensure(&state, "chat-b")
                .unwrap()
                .selection
                .unwrap()
                .provider_profile_id,
            "local-b"
        );
        assert_eq!(
            profile_for_choice(&state, a.selection.as_ref().unwrap())
                .unwrap()
                .base_url,
            "http://127.0.0.1:1234/v1"
        );
    }

    #[test]
    fn endpoint_reconnect_does_not_reuse_a_different_credential_owner() {
        let (_dir, state) = setup();
        let saved = ensure(&state, "chat-a").unwrap();
        state
            .session_api_keys
            .lock()
            .unwrap()
            .insert("local-a".into(), "synthetic-key-a".into());
        let mut request = ModelEndpointDraft {
            provider: ProviderKind::LmStudio,
            label: "Local A".into(),
            base_url: "http://127.0.0.1:1234/v1".into(),
            model: "model-a".into(),
            api_key: Some("synthetic-key-a".into()),
        };
        assert_eq!(
            local_connection_profile_id(&state, &request).unwrap(),
            "local-a"
        );
        request.api_key = Some("synthetic-key-b".into());
        assert_ne!(
            local_connection_profile_id(&state, &request).unwrap(),
            "local-a"
        );
        request.api_key = None;
        assert_ne!(
            local_connection_profile_id(&state, &request).unwrap(),
            "local-a"
        );
        request.api_key = Some(" ".into());
        assert_ne!(
            local_connection_profile_id(&state, &request).unwrap(),
            "local-a"
        );
        let profile = profile_for_choice(&state, saved.selection.as_ref().unwrap()).unwrap();
        assert_eq!(
            config_from_profile(&state, &profile)
                .unwrap()
                .api_key
                .as_deref(),
            Some("synthetic-key-a")
        );
        assert_eq!(
            state.storage.conversation_composer("chat-a").unwrap(),
            saved
        );
    }
    #[test]
    fn choosing_a_model_preserves_draft_other_chat_and_default() {
        let (_dir, state) = setup();
        let a = ensure(&state, "chat-a").unwrap();
        let b = ensure(&state, "chat-b").unwrap();
        let a = state
            .storage
            .save_conversation_composer("chat-a", a.revision, "unsent draft", a.selection.as_ref())
            .unwrap();
        state
            .storage
            .set_setting(
                &catalog_key("local-a"),
                &LocalCatalog {
                    base_url: "http://127.0.0.1:1234/v1".into(),
                    provider_kind: "lm-studio".into(),
                    models: vec!["model-a".into(), "model-b".into()],
                },
            )
            .unwrap();
        let selected = choose(
            &state,
            ChoiceRequest {
                conversation_id: "chat-a".into(),
                revision: a.revision,
                selection: ConversationModelChoice {
                    provider_profile_id: "local-a".into(),
                    model: "model-b".into(),
                    reasoning_effort: None,
                },
            },
        )
        .unwrap();
        assert_eq!(selected.composer.draft, "unsent draft");
        assert_eq!(selected.connection.unwrap().model, "model-b");
        assert_eq!(state.storage.conversation_composer("chat-b").unwrap(), b);
        assert_eq!(
            state
                .storage
                .default_provider_profile()
                .unwrap()
                .unwrap()
                .model,
            "model-a"
        );
    }
    #[test]
    fn unavailable_model_or_unsupported_effort_does_not_replace_choice() {
        let (_dir, state) = setup();
        let before = ensure(&state, "chat-a").unwrap();
        for (model, effort) in [("missing", None), ("model-a", Some("ultra".into()))] {
            assert!(choose(
                &state,
                ChoiceRequest {
                    conversation_id: "chat-a".into(),
                    revision: before.revision,
                    selection: ConversationModelChoice {
                        provider_profile_id: "local-a".into(),
                        model: model.into(),
                        reasoning_effort: effort
                    }
                }
            )
            .is_err());
        }
        assert_eq!(
            state.storage.conversation_composer("chat-a").unwrap(),
            before
        );
    }
    #[test]
    fn removed_connection_is_disconnected_without_falling_back_to_global_default() {
        let (_dir, state) = setup();
        let a = ensure(&state, "chat-a").unwrap();
        state.storage.delete_provider_profile("local-a").unwrap();
        let result = snapshot(&state, a).unwrap();
        assert!(result.connection.is_none());
        assert!(result.warning.unwrap().contains("removed"));
    }

    #[test]
    fn response_identity_comes_from_the_submitted_task_not_the_latest_picker() {
        let (_dir, state) = setup();
        let saved = ensure(&state, "chat-a").unwrap();
        state
            .storage
            .create_task(&TaskInput {
                id: "original-task".into(),
                conversation_id: Some("chat-a".into()),
                kind: "agent-turn".into(),
                payload: json!({"modelSelection":saved.selection}),
            })
            .unwrap();
        let mut next = saved.selection.clone().unwrap();
        next.model = "later-model".into();
        state
            .storage
            .save_conversation_composer("chat-a", saved.revision, "next draft", Some(&next))
            .unwrap();
        let input = assistant_message_input(
            &state.storage,
            "chat-a",
            "original-task",
            "Completed reply",
            Some("reported-snapshot"),
        )
        .unwrap();
        let message = state.storage.append_message(&input).unwrap();
        let view = message_view(message).unwrap();
        assert_eq!(view.requested_model.as_deref(), Some("model-a"));
        assert_eq!(view.reported_model.as_deref(), Some("reported-snapshot"));
    }

    #[cfg(windows)]
    #[test]
    fn membership_chat_choice_does_not_change_account_or_app_defaults() {
        use crate::membership::{
            protect_credentials, MembershipCatalog, MembershipCredentials, MembershipIdentity,
            MembershipModel,
        };
        let (_dir, state) = setup();
        let saved = ensure(&state, "chat-a").unwrap();
        let identity = MembershipIdentity {
            provider: "chatgpt".into(),
            issuer: "https://auth.openai.com".into(),
            subject: "synthetic-composer-user".into(),
            client_id: "synthetic-composer-client".into(),
            host_id: state.storage.membership_host_id().unwrap(),
            email: None,
        };
        let credentials = MembershipCredentials {
            issuer: identity.issuer.clone(),
            subject: identity.subject.clone(),
            client_id: identity.client_id.clone(),
            ext_agent_host_id: identity.host_id.clone(),
            id_token: "synthetic-id".into(),
            access_token: "synthetic-access".into(),
            refresh_token: "synthetic-refresh".into(),
            token_type: "Bearer".into(),
            scopes: vec![
                "openid".into(),
                "resource.invoke".into(),
                "chatgpt.tokens.use.direct".into(),
            ],
            access_expires_at_ms: 1_900_000_000_000,
            codex_images: None,
        };
        let account = state
            .storage
            .membership_add_account(
                "Synthetic account",
                &protect_credentials(&identity, &credentials).unwrap(),
            )
            .unwrap();
        state
            .storage
            .membership_save_catalog(
                &account.id,
                account.credential_version,
                &MembershipCatalog {
                    account_id: account.id.clone(),
                    models: vec![MembershipModel {
                        slug: "offered".into(),
                        display_name: "Offered model".into(),
                        reasoning_efforts: vec!["low".into(), "high".into()],
                    }],
                    fetched_at_ms: 1_800_000_000_000,
                },
            )
            .unwrap();
        let selected = choose(
            &state,
            ChoiceRequest {
                conversation_id: "chat-a".into(),
                revision: saved.revision,
                selection: ConversationModelChoice {
                    provider_profile_id: format!("membership:{}", account.id),
                    model: "offered".into(),
                    reasoning_effort: Some("low".into()),
                },
            },
        )
        .unwrap();
        assert_eq!(selected.connection.unwrap().model, "offered");
        assert_eq!(
            state
                .storage
                .default_provider_profile()
                .unwrap()
                .unwrap()
                .id,
            "local-a"
        );
        assert!(state
            .storage
            .membership_account(&account.id)
            .unwrap()
            .selection
            .is_none());
        state
            .storage
            .membership_clear_credentials(&account.id, account.credential_version)
            .unwrap();
        assert!(snapshot(&state, selected.composer)
            .unwrap()
            .connection
            .is_none());
    }
}
