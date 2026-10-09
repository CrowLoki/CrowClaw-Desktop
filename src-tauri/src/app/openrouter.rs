use super::*;
use crate::openrouter::{FreeCatalog, FreeModel, OpenRouterProvider, BASE_URL};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogRequest {
    pub profile_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectRequest {
    pub label: String,
    pub api_key: String,
    pub model: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisconnectRequest {
    pub profile_id: String,
}

fn cache_key(id: &str) -> String {
    format!("openrouter-free-catalog:{id}")
}
pub(super) fn cached_catalog(storage: &Storage, id: &str) -> Result<FreeCatalog, String> {
    storage
        .get_setting(&cache_key(id))
        .map_err(display_error)?
        .ok_or_else(|| "Refresh the OpenRouter free-model catalog before choosing a model".into())
}
pub(super) fn cached_model(
    state: &AppState,
    profile: &ProviderProfile,
    effort: Option<&str>,
) -> Result<FreeModel, String> {
    if !state
        .storage
        .has_openrouter_key(&profile.id)
        .map_err(display_error)?
    {
        return Err(
            "This OpenRouter connection is disconnected. Enter your own API key in Connections"
                .into(),
        );
    }
    let model = cached_catalog(&state.storage, &profile.id)?
        .models
        .into_iter()
        .find(|model| model.id == profile.model)
        .ok_or("This model is no longer in the free catalog. Refresh and select a free model")?;
    if effort.is_some_and(|value| {
        !model
            .reasoning_efforts
            .iter()
            .any(|offered| offered == value)
    }) {
        return Err(
            "This OpenRouter model has not advertised that reasoning level; use provider default"
                .into(),
        );
    }
    Ok(model)
}

#[tauri::command]
pub async fn crowclaw_openrouter_catalog(
    state: State<'_, AppState>,
    request: CatalogRequest,
) -> Result<FreeCatalog, String> {
    match request.profile_id {
        None => crate::openrouter::fetch_catalog(None, &CancellationToken::new())
            .await
            .map_err(display_error),
        Some(id) => refresh(&state, &id).await,
    }
}

pub(super) async fn refresh(state: &AppState, id: &str) -> Result<FreeCatalog, String> {
    let profile = state
        .storage
        .get_provider_profile(id)
        .map_err(display_error)?
        .ok_or("OpenRouter connection was not found")?;
    if profile.provider_kind != "openrouter" || profile.base_url != BASE_URL {
        return Err("Not an OpenRouter connection".into());
    }
    let key = state
        .storage
        .load_openrouter_key(id)
        .map_err(display_error)?
        .ok_or("OpenRouter is disconnected; enter your API key")?;
    let catalog = crate::openrouter::fetch_catalog(Some(&key), &CancellationToken::new())
        .await
        .map_err(display_error)?;
    if state
        .storage
        .load_openrouter_key(id)
        .map_err(display_error)?
        .as_deref()
        != Some(&key)
    {
        return Err(
            "OpenRouter connection changed during refresh; retry using its current state".into(),
        );
    }
    state
        .storage
        .set_setting(&cache_key(id), &catalog)
        .map_err(display_error)?;
    Ok(catalog)
}

#[tauri::command]
pub async fn crowclaw_openrouter_connect(
    state: State<'_, AppState>,
    request: ConnectRequest,
) -> Result<ModelConnection, String> {
    let key = request.api_key.trim();
    if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
        return Err("Enter a valid OpenRouter API key; it stays protected on this device".into());
    }
    let label = request.label.trim();
    if label.is_empty() || label.len() > 128 || label.chars().any(char::is_control) {
        return Err("Use a connection name of 1–128 bytes without control characters".into());
    }
    let catalog = crate::openrouter::fetch_catalog(Some(key), &CancellationToken::new())
        .await
        .map_err(display_error)?;
    if !catalog.models.iter().any(|model| model.id == request.model) {
        return Err("Choose a model currently offered free for your OpenRouter account".into());
    }
    let id = format!("openrouter:{}", Uuid::new_v4());
    let profile = state
        .storage
        .save_openrouter_connection_with_catalog(
            &ProviderProfileInput {
                id: id.clone(),
                name: label.into(),
                base_url: BASE_URL.into(),
                model: request.model,
                provider_kind: "openrouter".into(),
                credential_reference: Some(id.clone()),
                is_default: true,
            },
            key,
            &serde_json::to_value(&catalog).map_err(display_error)?,
        )
        .map_err(display_error)?;
    Ok(connection_view(&profile, "connected", None))
}

#[tauri::command]
pub async fn crowclaw_openrouter_disconnect(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: DisconnectRequest,
) -> Result<(), String> {
    let profile = state
        .storage
        .get_provider_profile(&request.profile_id)
        .map_err(display_error)?
        .ok_or("OpenRouter connection was not found")?;
    if profile.provider_kind != "openrouter" {
        return Err("Not an OpenRouter connection".into());
    }
    state
        .storage
        .remove_openrouter_key(&profile.id)
        .map_err(display_error)?;
    let ids: Vec<String> = state
        .active_tasks
        .lock()
        .map_err(|_| "Active task lock was poisoned")?
        .keys()
        .cloned()
        .collect();
    for id in ids {
        let task = state.storage.get_task(&id).map_err(display_error)?;
        if task
            .as_ref()
            .and_then(|task| task.payload.get("providerSnapshot"))
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            == Some(&profile.id)
        {
            let cancelled = cancel_task_core(&state, &id).await?;
            if cancelled.newly_cancelled {
                emit_task(&app, &state.storage, &cancelled.task)?;
            }
        }
    }
    Ok(())
}

struct ConnectedProvider {
    storage: Arc<Storage>,
    profile_id: String,
    inner: OpenRouterProvider,
}
#[async_trait::async_trait]
impl ChatProvider for ConnectedProvider {
    async fn complete(
        &self,
        request: crate::agent::ChatCompletionRequest,
        cancellation: &CancellationToken,
    ) -> Result<crate::agent::ChatCompletion, crate::agent::ProviderError> {
        if !self
            .storage
            .has_openrouter_key(&self.profile_id)
            .map_err(|_| crate::agent::ProviderError::InvalidConfiguration {
                message: "OpenRouter credential storage is unavailable".into(),
            })?
        {
            return Err(crate::agent::ProviderError::InvalidConfiguration {
                message: "OpenRouter was disconnected; no request was sent".into(),
            });
        }
        self.inner.complete(request, cancellation).await
    }
}

pub(super) fn provider(
    state: &AppState,
    profile: &ProviderProfile,
    effort: Option<String>,
) -> Result<Arc<dyn ChatProvider>, String> {
    cached_model(state, profile, effort.as_deref())?;
    let key = state
        .storage
        .load_openrouter_key(&profile.id)
        .map_err(display_error)?
        .ok_or("OpenRouter is disconnected")?;
    let inner =
        OpenRouterProvider::new(key, profile.model.clone(), effort).map_err(display_error)?;
    Ok(Arc::new(ConnectedProvider {
        storage: state.storage.clone(),
        profile_id: profile.id.clone(),
        inner,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, AppState, ProviderProfile) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::open(dir.path().into()).unwrap();
        let profile = state
            .storage
            .save_openrouter_connection(
                &ProviderProfileInput {
                    id: "openrouter-test".into(),
                    name: "Test free connection".into(),
                    base_url: BASE_URL.into(),
                    model: "fixture/free".into(),
                    provider_kind: "openrouter".into(),
                    credential_reference: Some("openrouter-test".into()),
                    is_default: true,
                },
                "synthetic-test-key",
            )
            .unwrap();
        state
            .storage
            .set_setting(
                &cache_key(&profile.id),
                &FreeCatalog {
                    fetched_at_ms: 1,
                    models: vec![FreeModel {
                        id: "fixture/free".into(),
                        name: "Fixture free".into(),
                        context_length: 4096,
                        input_modalities: vec!["text".into()],
                        supported_parameters: vec![],
                        reasoning_efforts: vec![],
                    }],
                },
            )
            .unwrap();
        (dir, state, profile)
    }
    #[test]
    fn openrouter_choices_require_saved_key_and_offered_model_and_effort() {
        let (_dir, state, mut profile) = setup();
        assert!(cached_model(&state, &profile, None).is_ok());
        assert!(cached_model(&state, &profile, Some("high")).is_err());
        profile.model = "not-offered".into();
        assert!(cached_model(&state, &profile, None).is_err());
        profile.model = "fixture/free".into();
        state.storage.remove_openrouter_key(&profile.id).unwrap();
        assert!(cached_model(&state, &profile, None).is_err());
        assert!(state
            .storage
            .get_provider_profile(&profile.id)
            .unwrap()
            .is_some());
    }
    #[test]
    fn new_chat_seeds_openrouter_without_mutating_an_existing_chat_choice() {
        let (_dir, state, profile) = setup();
        state
            .storage
            .create_conversation(&ConversationInput {
                id: "new".into(),
                title: "New".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let saved = composer::ensure(&state, "new").unwrap();
        assert_eq!(
            saved.selection.as_ref().unwrap().provider_profile_id,
            profile.id
        );
        let view = connection_view(&profile, "connected", None);
        assert_eq!(view.provider, "openrouter");
        let (_, provider) =
            composer::provider_for_choice(&state, saved.selection.as_ref().unwrap()).unwrap();
        drop(provider); // Construction is native-only; no provider request is made.
        assert_eq!(state.storage.conversation_composer("new").unwrap(), saved);
    }
    #[tokio::test]
    async fn custom_endpoint_cannot_bypass_openrouter_free_guard() {
        for url in [
            "https://openrouter.ai/api/v1",
            "https://eu.openrouter.ai/api/v1",
            "https://openrouter.ai./api/v1",
            "https://eu.openrouter.ai./api/v1",
        ] {
            assert!(is_openrouter_url(url));
            let result = test_connection(&ModelEndpointDraft {
                provider: ProviderKind::Custom,
                label: "Old route".into(),
                base_url: url.into(),
                model: "paid/model".into(),
                api_key: Some("synthetic-key".into()),
            })
            .await;
            assert!(result.unwrap_err().contains("free-model"));
        }
        assert!(!is_openrouter_url("https://openrouter.ai.example.test/v1"));
        assert!(!is_openrouter_url("http://127.0.0.1:1234/v1"));
    }
}
