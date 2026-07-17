use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, State};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    agent::{
        AgentLimits, AgentRunOutcome, AgentRuntime, AgentSession, CancellationToken, ChatMessage,
        ChatRole, OpenAiCompatibleClient, PendingToolCall, ProviderConfig, ProviderHealthState,
        ProviderPreset,
    },
    crowquant_memory::{
        agent_memory_id, remembered_memory, CrowQuantMemoryService,
        CrowQuantSearchHit as ServiceCrowQuantSearchHit,
    },
    storage::{
        ActionStatus as StoredActionStatus, ConversationInput,
        CrowQuantMemory as StoredCrowQuantMemory, Message, MessageInput,
        MessageRole as StoredMessageRole, ProposedAction as StoredAction, ProposedActionInput,
        ProviderProfile, ProviderProfileInput, Storage, StorageError, StoredTask, TaskInput,
        TaskStatus as StoredTaskStatus,
    },
    tools::{
        ActionId, ApprovalDecision, ProposedAction as RuntimeAction, ToolExecution, ToolExecutor,
        ToolOutput, ToolPolicy,
    },
};

const SETTINGS_KEY: &str = "app_settings";
const DEFAULT_PROVIDER_ID: &str = "crowclaw-default-provider";
const TASK_EVENT: &str = "crowclaw://task-updated";
const SYSTEM_PROMPT: &str = "You are CrowClaw, a local-first desktop AI agent. Be direct and useful. Use the supplied tools when the user asks to inspect a selected folder, run a local task, explicitly remember text, or search remembered text. CrowQuant memory retrieval is compressed lexical similarity, not a neural or semantic embedding. Never claim a tool ran until its actual returned tool result is present. The desktop application holds every tool action for explicit user approval.";

pub struct AppState {
    storage: Arc<Storage>,
    crowquant: Arc<CrowQuantMemoryService>,
    selected_folders: Mutex<HashMap<String, PathBuf>>,
    active_tasks: Mutex<HashMap<String, Arc<LiveTask>>>,
    action_to_task: Mutex<HashMap<String, String>>,
    session_api_keys: Mutex<HashMap<String, String>>,
}

struct LiveTask {
    runtime: Arc<AgentRuntime>,
    session: AsyncMutex<AgentSession>,
    cancellation: CancellationToken,
    conversation_id: String,
}

impl AppState {
    pub fn open(app_data_directory: PathBuf) -> Result<Self, StorageError> {
        let storage = Arc::new(Storage::open(app_data_directory)?);
        let crowquant = Arc::new(CrowQuantMemoryService::new(storage.clone()));

        // Approval tokens are intentionally process-local. Reconcile stale work
        // safely rather than exposing an approval button that cannot execute.
        reconcile_actions_after_restart(&storage)?;
        for task in storage.list_tasks(None)? {
            if matches!(
                task.status,
                StoredTaskStatus::Queued | StoredTaskStatus::Running
            ) {
                let _ = storage.update_task_status(
                    &task.id,
                    StoredTaskStatus::Failed,
                    None,
                    Some("Interrupted by application restart"),
                );
            }
        }

        Ok(Self {
            storage,
            crowquant,
            selected_folders: Mutex::new(HashMap::new()),
            active_tasks: Mutex::new(HashMap::new()),
            action_to_task: Mutex::new(HashMap::new()),
            session_api_keys: Mutex::new(HashMap::new()),
        })
    }
}

fn reconcile_actions_after_restart(storage: &Storage) -> Result<(), StorageError> {
    for action in storage.list_proposed_actions(None, Some(StoredActionStatus::Pending))? {
        storage.deny_action(&action.id, Some("Interrupted by application restart"))?;
    }
    for action in storage.list_proposed_actions(None, Some(StoredActionStatus::Approved))? {
        if action.tool_name == "remember_memory" {
            if let Ok(action_id) = ActionId::parse(&action.id) {
                let memory_id = agent_memory_id(&action_id);
                if let Some(memory) = storage.get_crowquant_memory(&memory_id)? {
                    let execution = ToolExecution::Executed {
                        action_id,
                        output: ToolOutput::MemoryRemembered {
                            memory: remembered_memory(memory),
                        },
                    };
                    storage.record_action_success(&action.id, &serde_json::to_value(execution)?)?;
                    continue;
                }
            }
        }
        storage.record_action_failure(
            &action.id,
            "Application stopped before the approved action returned a durable result",
        )?;
    }
    Ok(())
}

fn interrupt_unexecuted_action(
    storage: &Storage,
    action_id: &str,
    reason: &str,
) -> Result<(), StorageError> {
    let Some(action) = storage.get_proposed_action(action_id)? else {
        return Ok(());
    };
    match action.status {
        StoredActionStatus::Pending => {
            storage.deny_action(action_id, Some(reason))?;
        }
        StoredActionStatus::Approved => {
            storage.record_action_failure(action_id, reason)?;
        }
        StoredActionStatus::Denied | StoredActionStatus::Succeeded | StoredActionStatus::Failed => {
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    LmStudio,
    Ollama,
    LlamaCpp,
    Custom,
}

impl ProviderKind {
    fn preset(&self) -> ProviderPreset {
        match self {
            Self::LmStudio => ProviderPreset::LmStudio,
            Self::Ollama => ProviderPreset::Ollama,
            Self::LlamaCpp => ProviderPreset::LlamaCpp,
            Self::Custom => ProviderPreset::Custom,
        }
    }

    fn storage_name(&self) -> &'static str {
        match self {
            Self::LmStudio => "lm-studio",
            Self::Ollama => "ollama",
            Self::LlamaCpp => "llama-cpp",
            Self::Custom => "custom",
        }
    }

    fn from_storage(value: &str) -> Self {
        match value {
            "lm-studio" => Self::LmStudio,
            "ollama" => Self::Ollama,
            "llama-cpp" => Self::LlamaCpp,
            _ => Self::Custom,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEndpointDraft {
    provider: ProviderKind,
    label: String,
    base_url: String,
    model: String,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConnection {
    id: String,
    provider: ProviderKind,
    label: String,
    base_url: String,
    model: String,
    status: &'static str,
    connected_at: Option<String>,
    latency_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredEndpoint {
    id: String,
    provider: ProviderKind,
    label: String,
    base_url: String,
    model: String,
    detected: bool,
    available_models: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    ok: bool,
    latency_ms: Option<u64>,
    resolved_model: Option<String>,
    detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    id: String,
    role: &'static str,
    content: String,
    created_at: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    id: String,
    title: String,
    preview: String,
    updated_at: String,
    unread: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    id: String,
    title: String,
    created_at: String,
    updated_at: String,
    messages: Vec<ConversationMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedFolder {
    id: String,
    name: String,
    display_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskView {
    id: String,
    conversation_id: String,
    title: String,
    detail: String,
    status: &'static str,
    progress: Option<u8>,
    started_at: String,
    updated_at: String,
    cancellable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingActionView {
    id: String,
    task_id: String,
    conversation_id: String,
    kind: &'static str,
    title: String,
    summary: String,
    target: String,
    details: Vec<String>,
    risk: &'static str,
    requested_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    id: String,
    title: String,
    preview: String,
    source: &'static str,
    conversation_id: Option<String>,
    created_at: String,
    tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrowQuantMemoryView {
    id: String,
    text: String,
    created_at: String,
    original_bytes: u64,
    compressed_bytes: u64,
    compression_ratio: f64,
    algorithm: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrowQuantSearchHit {
    memory: CrowQuantMemoryView,
    score: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrowQuantRememberRequest {
    text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrowQuantRecallRequest {
    query: String,
    #[serde(default = "default_crowquant_limit")]
    limit: usize,
}

fn default_crowquant_limit() -> usize {
    5
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    Ask,
    AllowSession,
    Deny,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSettings {
    read_files: PermissionMode,
    write_files: PermissionMode,
    run_commands: PermissionMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    permissions: PermissionSettings,
    launch_at_login: bool,
    keep_running_on_close: bool,
    retain_conversations: bool,
    theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            permissions: PermissionSettings {
                read_files: PermissionMode::Ask,
                write_files: PermissionMode::Ask,
                run_commands: PermissionMode::Ask,
            },
            launch_at_login: false,
            keep_running_on_close: false,
            retain_conversations: true,
            theme: "dark".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    first_run: bool,
    connection: Option<ModelConnection>,
    conversations: Vec<ConversationSummary>,
    selected_conversation_id: Option<String>,
    tasks: Vec<AgentTaskView>,
    pending_actions: Vec<PendingActionView>,
    memories: Vec<MemoryRecord>,
    settings: AppSettings,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCreated {
    conversation: ConversationView,
    summary: ConversationSummary,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurnResult {
    conversation: ConversationView,
    summary: ConversationSummary,
    task: AgentTaskView,
    pending_actions: Vec<PendingActionView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDecisionResult {
    conversation: ConversationView,
    summary: ConversationSummary,
    task: AgentTaskView,
    pending_actions: Vec<PendingActionView>,
    memory: Option<MemoryRecord>,
    memories: Vec<MemoryRecord>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCancellationResult {
    task: AgentTaskView,
    conversation: Option<ConversationView>,
    summary: Option<ConversationSummary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRequest {
    conversation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSendRequest {
    conversation_id: String,
    content: String,
    selected_folder: Option<SelectedFolder>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequest {
    task_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionDecisionRequest {
    Approved,
    Denied,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideActionRequest {
    action_id: String,
    decision: ActionDecisionRequest,
}

#[tauri::command]
pub fn crowclaw_app_bootstrap(state: State<'_, AppState>) -> Result<AppBootstrap, String> {
    bootstrap(&state).map_err(display_error)
}

#[tauri::command]
pub async fn crowclaw_model_discover(
    _state: State<'_, AppState>,
) -> Result<Vec<DiscoveredEndpoint>, String> {
    let presets = [
        (
            ProviderKind::LmStudio,
            "LM Studio",
            "http://127.0.0.1:1234/v1",
        ),
        (ProviderKind::Ollama, "Ollama", "http://127.0.0.1:11434/v1"),
        (
            ProviderKind::LlamaCpp,
            "llama.cpp",
            "http://127.0.0.1:8080/v1",
        ),
    ];
    let mut discovered = Vec::new();
    for (provider, label, base_url) in presets {
        let draft = ModelEndpointDraft {
            provider: provider.clone(),
            label: label.into(),
            base_url: base_url.into(),
            model: "local-model".into(),
            api_key: None,
        };
        let mut config = config_for(&draft);
        config.request_timeout_ms = 1_200;
        let Ok(client) = OpenAiCompatibleClient::new(config) else {
            continue;
        };
        if let Ok(models) = client.list_models(&CancellationToken::new()).await {
            let available_models = models.into_iter().map(|model| model.id).collect::<Vec<_>>();
            let model = available_models
                .first()
                .cloned()
                .unwrap_or_else(|| "local-model".into());
            discovered.push(DiscoveredEndpoint {
                id: format!("detected-{}", provider.storage_name()),
                provider,
                label: label.into(),
                base_url: base_url.into(),
                model,
                detected: true,
                available_models,
            });
        }
    }
    Ok(discovered)
}

#[tauri::command]
pub async fn crowclaw_model_test_connection(
    request: ModelEndpointDraft,
) -> Result<ConnectionTestResult, String> {
    test_connection(&request).await
}

#[tauri::command]
pub async fn crowclaw_model_connect(
    state: State<'_, AppState>,
    request: ModelEndpointDraft,
) -> Result<ModelConnection, String> {
    let tested = test_connection(&request).await?;
    if !tested.ok {
        return Err(tested.detail);
    }
    let profile = state
        .storage
        .save_provider_profile(&ProviderProfileInput {
            id: DEFAULT_PROVIDER_ID.into(),
            name: non_empty_or(&request.label, "Local model"),
            base_url: request.base_url.trim().trim_end_matches('/').into(),
            model: tested
                .resolved_model
                .clone()
                .unwrap_or_else(|| request.model.trim().into()),
            provider_kind: request.provider.storage_name().into(),
            credential_reference: None,
            is_default: true,
        })
        .map_err(display_error)?;
    if let Some(api_key) = request.api_key.filter(|value| !value.trim().is_empty()) {
        state
            .session_api_keys
            .lock()
            .map_err(|_| "API-key session lock was poisoned".to_string())?
            .insert(profile.id.clone(), api_key);
    }
    Ok(connection_view(&profile, "connected", tested.latency_ms))
}

#[tauri::command]
pub fn crowclaw_conversation_create(
    state: State<'_, AppState>,
) -> Result<ConversationCreated, String> {
    let provider = state
        .storage
        .default_provider_profile()
        .map_err(display_error)?;
    let conversation = state
        .storage
        .create_conversation(&ConversationInput {
            id: Uuid::new_v4().to_string(),
            title: "New conversation".into(),
            provider_profile_id: provider.map(|profile| profile.id),
        })
        .map_err(display_error)?;
    state
        .storage
        .set_setting("selected_conversation_id", &conversation.id)
        .map_err(display_error)?;
    let view = conversation_view(&state.storage, &conversation.id).map_err(display_error)?;
    Ok(ConversationCreated {
        summary: summary_for(&view),
        conversation: view,
    })
}

#[tauri::command]
pub fn crowclaw_conversation_get(
    state: State<'_, AppState>,
    request: ConversationRequest,
) -> Result<ConversationView, String> {
    let view =
        conversation_view(&state.storage, &request.conversation_id).map_err(display_error)?;
    state
        .storage
        .set_setting("selected_conversation_id", &request.conversation_id)
        .map_err(display_error)?;
    Ok(view)
}

#[tauri::command]
pub async fn crowclaw_folder_select(
    state: State<'_, AppState>,
) -> Result<Option<SelectedFolder>, String> {
    let picked = tauri::async_runtime::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .map_err(|error| format!("Folder picker failed: {error}"))?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Could not resolve selected folder: {error}"))?;
    let id = Uuid::new_v4().to_string();
    state
        .selected_folders
        .lock()
        .map_err(|_| "Selected-folder lock was poisoned".to_string())?
        .insert(id.clone(), canonical.clone());
    Ok(Some(SelectedFolder {
        id,
        name: canonical
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| canonical.display().to_string()),
        display_path: canonical.display().to_string(),
    }))
}

#[tauri::command]
pub fn crowclaw_crowquant_list(
    state: State<'_, AppState>,
) -> Result<Vec<CrowQuantMemoryView>, String> {
    state
        .crowquant
        .list_records()
        .map(|records| records.iter().map(crowquant_memory_view).collect())
}

#[tauri::command]
pub fn crowclaw_crowquant_remember(
    state: State<'_, AppState>,
    request: CrowQuantRememberRequest,
) -> Result<CrowQuantMemoryView, String> {
    let record = state.crowquant.remember_record(&request.text)?;
    Ok(crowquant_memory_view(&record))
}

#[tauri::command]
pub fn crowclaw_crowquant_recall(
    state: State<'_, AppState>,
    request: CrowQuantRecallRequest,
) -> Result<Vec<CrowQuantSearchHit>, String> {
    state
        .crowquant
        .search_records(&request.query, request.limit)
        .map(|hits| hits.into_iter().map(crowquant_search_hit_view).collect())
}

#[tauri::command]
pub async fn crowclaw_chat_send(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ChatSendRequest,
) -> Result<ChatTurnResult, String> {
    let content = request.content.trim();
    if content.is_empty() {
        return Err("Message cannot be empty".into());
    }
    let conversation = state
        .storage
        .get_conversation(&request.conversation_id)
        .map_err(display_error)?
        .ok_or_else(|| "Conversation was not found".to_string())?;
    let previous = state
        .storage
        .list_messages(&conversation.id)
        .map_err(display_error)?;
    let first_user_message = !previous
        .iter()
        .any(|message| message.role == StoredMessageRole::User);
    state
        .storage
        .append_message(&MessageInput {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            role: StoredMessageRole::User,
            content: content.into(),
            metadata: Value::Null,
        })
        .map_err(display_error)?;
    if first_user_message {
        state
            .storage
            .update_conversation(&ConversationInput {
                id: conversation.id.clone(),
                title: title_from(content),
                provider_profile_id: conversation.provider_profile_id.clone(),
            })
            .map_err(display_error)?;
    }
    state
        .storage
        .set_setting("selected_conversation_id", &conversation.id)
        .map_err(display_error)?;

    let provider_profile = state
        .storage
        .default_provider_profile()
        .map_err(display_error)?
        .ok_or_else(|| "Connect a local model before sending a message".to_string())?;
    let provider = Arc::new(
        OpenAiCompatibleClient::new(config_from_profile(&state, &provider_profile)?)
            .map_err(display_error)?,
    );
    let settings = load_settings(&state.storage).map_err(display_error)?;
    let selected_root = match &request.selected_folder {
        Some(folder) => Some(
            state
                .selected_folders
                .lock()
                .map_err(|_| "Selected-folder lock was poisoned".to_string())?
                .get(&folder.id)
                .cloned()
                .ok_or_else(|| {
                    "Select the folder again before granting local access".to_string()
                })?,
        ),
        None => None,
    };
    let mut policy = if settings.permissions.read_files == PermissionMode::Deny {
        ToolPolicy::default()
    } else {
        ToolPolicy::for_roots(selected_root.clone().into_iter())
    };
    policy.allow_commands = settings.permissions.run_commands != PermissionMode::Deny;
    let runtime = Arc::new(
        AgentRuntime::new(
            provider,
            ToolExecutor::new(policy)
                .map_err(display_error)?
                .with_memory_backend(state.crowquant.clone()),
            AgentLimits::default(),
        )
        .map_err(display_error)?,
    );

    let mut messages = vec![ChatMessage::system(SYSTEM_PROMPT)];
    messages.extend(previous.iter().filter_map(stored_to_agent_message));
    let model_content = match &selected_root {
        Some(path) => format!(
            "{content}\n\nUser-selected folder (access remains approval-gated): [path:{}]",
            path.display()
        ),
        None => content.into(),
    };
    messages.push(ChatMessage::user(model_content));
    let task_id = Uuid::new_v4().to_string();
    let task = state
        .storage
        .create_task(&TaskInput {
            id: task_id.clone(),
            conversation_id: Some(conversation.id.clone()),
            kind: "agent-turn".into(),
            payload: json!({
                "title": title_from(content),
                "detail": "Working with the connected local model",
                "selectedFolderId": request.selected_folder.as_ref().map(|folder| &folder.id),
            }),
        })
        .map_err(display_error)?;
    let running = state
        .storage
        .update_task_status(&task.id, StoredTaskStatus::Running, None, None)
        .map_err(display_error)?;
    let live = Arc::new(LiveTask {
        runtime,
        session: AsyncMutex::new(
            AgentSession::new(provider_profile.model.clone(), messages).map_err(display_error)?,
        ),
        cancellation: CancellationToken::new(),
        conversation_id: conversation.id.clone(),
    });
    state
        .active_tasks
        .lock()
        .map_err(|_| "Active-task lock was poisoned".to_string())?
        .insert(task_id.clone(), live.clone());
    emit_task(&app, &state.storage, &running)?;

    let outcome = {
        let mut session = live.session.lock().await;
        live.runtime
            .run_until_blocked(&mut session, &live.cancellation)
            .await
    };
    let pending_actions = match outcome {
        Ok(AgentRunOutcome::Completed { message, .. }) => {
            persist_assistant_message(&state.storage, &conversation.id, &task_id, &message)?;
            let completed = state
                .storage
                .update_task_status(
                    &task_id,
                    StoredTaskStatus::Succeeded,
                    Some(&json!({ "message": message.content })),
                    None,
                )
                .map_err(display_error)?;
            remove_live_task(&state, &task_id)?;
            emit_task(&app, &state.storage, &completed)?;
            Vec::new()
        }
        Ok(AgentRunOutcome::AwaitingApproval { actions, .. }) => {
            let views = persist_runtime_actions(&state, &conversation.id, &task_id, &actions)?;
            let current = state
                .storage
                .get_task(&task_id)
                .map_err(display_error)?
                .ok_or_else(|| "Task was not found".to_string())?;
            emit_task(&app, &state.storage, &current)?;
            views
        }
        Err(error) => {
            if matches!(error, crate::agent::AgentError::Cancelled) {
                remove_live_task(&state, &task_id)?;
                Vec::new()
            } else {
                let failed = state
                    .storage
                    .update_task_status(
                        &task_id,
                        StoredTaskStatus::Failed,
                        None,
                        Some(&error.to_string()),
                    )
                    .map_err(display_error)?;
                remove_live_task(&state, &task_id)?;
                emit_task(&app, &state.storage, &failed)?;
                return Err(error.to_string());
            }
        }
    };
    chat_result(&state.storage, &conversation.id, &task_id, pending_actions).map_err(display_error)
}

#[tauri::command]
pub async fn crowclaw_task_cancel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: TaskRequest,
) -> Result<TaskCancellationResult, String> {
    let live = state
        .active_tasks
        .lock()
        .map_err(|_| "Active-task lock was poisoned".to_string())?
        .get(&request.task_id)
        .cloned();
    if let Some(live) = &live {
        live.cancellation.cancel();
    }
    let task = state
        .storage
        .get_task(&request.task_id)
        .map_err(display_error)?
        .ok_or_else(|| "Task was not found".to_string())?;
    state
        .storage
        .request_task_cancellation(&task.id)
        .map_err(display_error)?;

    if let Some(live) = &live {
        let session = live.session.lock().await;
        for pending in &session.pending_actions {
            let action_id = pending.proposal.action_id.to_string();
            let stored = state
                .storage
                .get_proposed_action(&action_id)
                .map_err(display_error)?;
            if stored.as_ref().map(|action| action.status) == Some(StoredActionStatus::Pending) {
                live.runtime
                    .resolve_action(
                        &session,
                        &pending.proposal.approval_token,
                        ApprovalDecision::Deny {
                            reason: Some("Task cancelled by user".into()),
                        },
                    )
                    .map_err(display_error)?;
            }
            interrupt_unexecuted_action(&state.storage, &action_id, "Task cancelled by user")
                .map_err(display_error)?;
        }
    }
    let cancelled = state
        .storage
        .update_task_status(&task.id, StoredTaskStatus::Cancelled, None, None)
        .map_err(display_error)?;
    remove_live_task(&state, &task.id)?;
    let conversation_id = task
        .conversation_id
        .or_else(|| live.as_ref().map(|item| item.conversation_id.clone()));
    if let Some(conversation_id) = &conversation_id {
        state
            .storage
            .append_message(&MessageInput {
                id: Uuid::new_v4().to_string(),
                conversation_id: conversation_id.clone(),
                role: StoredMessageRole::Assistant,
                content: "Task cancelled. Completed approved actions remain recorded; every unexecuted action was closed.".into(),
                metadata: json!({ "taskId": task.id }),
            })
            .map_err(display_error)?;
    }
    emit_task(&app, &state.storage, &cancelled)?;
    let conversation = conversation_id
        .as_deref()
        .map(|id| conversation_view(&state.storage, id))
        .transpose()
        .map_err(display_error)?;
    let summary = conversation.as_ref().map(summary_for);
    Ok(TaskCancellationResult {
        task: task_view(&state.storage, &cancelled).map_err(display_error)?,
        conversation,
        summary,
    })
}

#[tauri::command]
pub async fn crowclaw_action_decide(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: DecideActionRequest,
) -> Result<ActionDecisionResult, String> {
    let task_id = state
        .action_to_task
        .lock()
        .map_err(|_| "Action-map lock was poisoned".to_string())?
        .get(&request.action_id)
        .cloned()
        .ok_or_else(|| "That action is no longer pending".to_string())?;
    let live = state
        .active_tasks
        .lock()
        .map_err(|_| "Active-task lock was poisoned".to_string())?
        .get(&task_id)
        .cloned()
        .ok_or_else(|| "The action task is no longer active".to_string())?;
    let mut session = live.session.lock().await;
    let pending_call = session
        .pending_actions
        .iter()
        .find(|pending| pending.proposal.action_id.to_string() == request.action_id)
        .cloned()
        .ok_or_else(|| "That action is no longer pending".to_string())?;
    let proposal = pending_call.proposal.clone();
    let pending_before_run = session.pending_actions.clone();

    let approved = matches!(request.decision, ActionDecisionRequest::Approved);
    if approved {
        state
            .storage
            .approve_action(&request.action_id, Some("Approved once by user"))
            .map_err(display_error)?;
        live.runtime
            .resolve_action(
                &session,
                &proposal.approval_token,
                ApprovalDecision::Approve,
            )
            .map_err(display_error)?;
    } else {
        state
            .storage
            .deny_action(&request.action_id, Some("Denied by user"))
            .map_err(display_error)?;
        live.runtime
            .resolve_action(
                &session,
                &proposal.approval_token,
                ApprovalDecision::Deny {
                    reason: Some("Denied by user".into()),
                },
            )
            .map_err(display_error)?;
    }

    let before_message_len = session.messages.len();
    let run = live
        .runtime
        .run_until_blocked(&mut session, &live.cancellation)
        .await;
    let recorded = record_tool_executions(
        &state.storage,
        &pending_before_run,
        &session.messages[before_message_len.min(session.messages.len())..],
    )?;
    let memories = recorded
        .iter()
        .map(|(_, memory)| memory.clone())
        .collect::<Vec<_>>();
    let memory = recorded
        .iter()
        .find(|(action_id, _)| action_id == &request.action_id)
        .or_else(|| recorded.first())
        .map(|(_, memory)| memory.clone());
    drop(session);
    state
        .action_to_task
        .lock()
        .map_err(|_| "Action-map lock was poisoned".to_string())?
        .remove(&request.action_id);

    let pending_actions = match run {
        Ok(AgentRunOutcome::Completed { message, .. }) => {
            persist_assistant_message(&state.storage, &live.conversation_id, &task_id, &message)?;
            let completed = state
                .storage
                .update_task_status(
                    &task_id,
                    StoredTaskStatus::Succeeded,
                    Some(&json!({ "message": message.content })),
                    None,
                )
                .map_err(display_error)?;
            remove_live_task(&state, &task_id)?;
            emit_task(&app, &state.storage, &completed)?;
            Vec::new()
        }
        Ok(AgentRunOutcome::AwaitingApproval { actions, .. }) => {
            let pending =
                persist_runtime_actions(&state, &live.conversation_id, &task_id, &actions)?;
            let current = state
                .storage
                .get_task(&task_id)
                .map_err(display_error)?
                .ok_or_else(|| "Task was not found".to_string())?;
            emit_task(&app, &state.storage, &current)?;
            pending
        }
        Err(error) => {
            state
                .storage
                .append_message(&MessageInput {
                    id: Uuid::new_v4().to_string(),
                    conversation_id: live.conversation_id.clone(),
                    role: StoredMessageRole::Assistant,
                    content: format!("The approved task stopped safely: {error}"),
                    metadata: json!({ "taskId": task_id }),
                })
                .map_err(display_error)?;
            let failed = state
                .storage
                .update_task_status(
                    &task_id,
                    StoredTaskStatus::Failed,
                    None,
                    Some(&error.to_string()),
                )
                .map_err(display_error)?;
            remove_live_task(&state, &task_id)?;
            emit_task(&app, &state.storage, &failed)?;
            Vec::new()
        }
    };

    let conversation =
        conversation_view(&state.storage, &live.conversation_id).map_err(display_error)?;
    let stored_task = state
        .storage
        .get_task(&task_id)
        .map_err(display_error)?
        .ok_or_else(|| "Task was not found".to_string())?;
    Ok(ActionDecisionResult {
        summary: summary_for(&conversation),
        conversation,
        task: task_view(&state.storage, &stored_task).map_err(display_error)?,
        pending_actions,
        memory,
        memories,
    })
}

#[tauri::command]
pub fn crowclaw_settings_save(
    state: State<'_, AppState>,
    request: AppSettings,
) -> Result<AppSettings, String> {
    state
        .storage
        .set_setting(SETTINGS_KEY, &request)
        .map_err(display_error)?;
    Ok(request)
}

fn bootstrap(state: &AppState) -> Result<AppBootstrap, StorageError> {
    let provider = state.storage.default_provider_profile()?;
    let conversation_records = state.storage.list_conversations(false)?;
    let mut conversations = Vec::with_capacity(conversation_records.len());
    for conversation in conversation_records {
        conversations.push(summary_for(&conversation_view(
            &state.storage,
            &conversation.id,
        )?));
    }
    let task_records = state.storage.list_tasks(None)?;
    let mut tasks = Vec::with_capacity(task_records.len());
    for task in &task_records {
        tasks.push(task_view(&state.storage, task)?);
    }
    let pending_actions = state
        .storage
        .list_proposed_actions(None, Some(StoredActionStatus::Pending))?
        .iter()
        .filter_map(pending_action_from_stored)
        .collect();
    let memories = state
        .storage
        .list_proposed_actions(None, Some(StoredActionStatus::Succeeded))?
        .iter()
        .map(memory_from_action)
        .collect();
    let selected_conversation_id = state
        .storage
        .get_setting::<String>("selected_conversation_id")?
        .filter(|selected| conversations.iter().any(|item| item.id == *selected))
        .or_else(|| conversations.first().map(|item| item.id.clone()));
    Ok(AppBootstrap {
        first_run: provider.is_none(),
        connection: provider
            .as_ref()
            .map(|profile| connection_view(profile, "connected", None)),
        conversations,
        selected_conversation_id,
        tasks,
        pending_actions,
        memories,
        settings: load_settings(&state.storage)?,
    })
}

async fn test_connection(request: &ModelEndpointDraft) -> Result<ConnectionTestResult, String> {
    if request.model.trim().is_empty() {
        return Ok(ConnectionTestResult {
            ok: false,
            latency_ms: None,
            resolved_model: None,
            detail: "Choose or enter a model name".into(),
        });
    }
    let client = OpenAiCompatibleClient::new(config_for(request)).map_err(display_error)?;
    let cancellation = CancellationToken::new();
    let health = client.health(&cancellation).await.map_err(display_error)?;
    let models = if health.state == ProviderHealthState::Unavailable {
        Vec::new()
    } else {
        client.list_models(&cancellation).await.unwrap_or_default()
    };
    let resolved_model = if models.iter().any(|model| model.id == request.model.trim()) {
        request.model.trim().into()
    } else {
        models
            .first()
            .map(|model| model.id.clone())
            .unwrap_or_else(|| request.model.trim().into())
    };
    let ok = health.state != ProviderHealthState::Unavailable;
    Ok(ConnectionTestResult {
        ok,
        latency_ms: Some(health.latency_ms),
        resolved_model: ok.then_some(resolved_model),
        detail: if ok {
            format!(
                "Connected to {}",
                non_empty_or(&request.label, "local endpoint")
            )
        } else {
            health.detail
        },
    })
}

fn config_for(request: &ModelEndpointDraft) -> ProviderConfig {
    ProviderConfig {
        preset: request.provider.preset(),
        base_url: request.base_url.clone(),
        api_key: request.api_key.clone(),
        default_model: Some(request.model.clone()),
        request_timeout_ms: 60_000,
        max_response_bytes: 4 * 1024 * 1024,
    }
}

fn config_from_profile(
    state: &AppState,
    profile: &ProviderProfile,
) -> Result<ProviderConfig, String> {
    let api_key = state
        .session_api_keys
        .lock()
        .map_err(|_| "API-key session lock was poisoned".to_string())?
        .get(&profile.id)
        .cloned();
    Ok(ProviderConfig {
        preset: ProviderKind::from_storage(&profile.provider_kind).preset(),
        base_url: profile.base_url.clone(),
        api_key,
        default_model: Some(profile.model.clone()),
        request_timeout_ms: 60_000,
        max_response_bytes: 4 * 1024 * 1024,
    })
}

fn load_settings(storage: &Storage) -> Result<AppSettings, StorageError> {
    Ok(storage
        .get_setting::<AppSettings>(SETTINGS_KEY)?
        .unwrap_or_default())
}

fn conversation_view(storage: &Storage, id: &str) -> Result<ConversationView, StorageError> {
    let conversation = storage
        .get_conversation(id)?
        .ok_or_else(|| StorageError::InvalidData(format!("conversation '{id}' was not found")))?;
    let messages = storage
        .list_messages(id)?
        .into_iter()
        .filter_map(message_view)
        .collect();
    Ok(ConversationView {
        id: conversation.id,
        title: conversation.title,
        created_at: iso(conversation.created_at_ms),
        updated_at: iso(conversation.updated_at_ms),
        messages,
    })
}

fn message_view(message: Message) -> Option<ConversationMessage> {
    let role = match message.role {
        StoredMessageRole::System => "system",
        StoredMessageRole::User => "user",
        StoredMessageRole::Assistant => "assistant",
        StoredMessageRole::Tool => return None,
    };
    Some(ConversationMessage {
        id: message.id,
        role,
        content: message.content,
        created_at: iso(message.created_at_ms),
        status: "sent",
        task_id: message
            .metadata
            .get("taskId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn summary_for(conversation: &ConversationView) -> ConversationSummary {
    ConversationSummary {
        id: conversation.id.clone(),
        title: conversation.title.clone(),
        preview: conversation
            .messages
            .last()
            .map(|message| message.content.clone())
            .unwrap_or_else(|| "No messages yet".into()),
        updated_at: conversation.updated_at.clone(),
        unread: false,
    }
}

fn task_view(storage: &Storage, task: &StoredTask) -> Result<AgentTaskView, StorageError> {
    let has_pending = if let Some(conversation_id) = task.conversation_id.as_deref() {
        storage
            .list_proposed_actions(Some(conversation_id), Some(StoredActionStatus::Pending))?
            .iter()
            .any(|action| action.task_id.as_deref() == Some(&task.id))
    } else {
        false
    };
    let status = match task.status {
        StoredTaskStatus::Queued => "queued",
        StoredTaskStatus::Running if has_pending => "waiting-approval",
        StoredTaskStatus::Running => "running",
        StoredTaskStatus::Succeeded => "completed",
        StoredTaskStatus::Failed => "failed",
        StoredTaskStatus::Cancelled => "cancelled",
    };
    let title = task
        .payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("CrowClaw task")
        .to_string();
    let detail = if let Some(error) = &task.error {
        error.clone()
    } else if has_pending {
        "Waiting for your approval".into()
    } else {
        task.payload
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("Working with the connected local model")
            .to_string()
    };
    Ok(AgentTaskView {
        id: task.id.clone(),
        conversation_id: task.conversation_id.clone().unwrap_or_default(),
        title,
        detail,
        status,
        progress: (task.status == StoredTaskStatus::Succeeded).then_some(100),
        started_at: iso(task.started_at_ms.unwrap_or(task.created_at_ms)),
        updated_at: iso(task.updated_at_ms),
        cancellable: matches!(
            task.status,
            StoredTaskStatus::Queued | StoredTaskStatus::Running
        ),
    })
}

fn pending_action_from_stored(action: &StoredAction) -> Option<PendingActionView> {
    let task_id = action.task_id.clone()?;
    Some(PendingActionView {
        id: action.id.clone(),
        task_id,
        conversation_id: action.conversation_id.clone(),
        kind: action_kind(&action.tool_name),
        title: action_title(&action.tool_name).into(),
        summary: action.summary.clone(),
        target: action_target(&action.tool_name, &action.request),
        details: action_details(&action.tool_name, &action.request),
        risk: match action.tool_name.as_str() {
            "run_command" => "high",
            "remember_memory" | "search_memory" => "medium",
            _ => "low",
        },
        requested_at: iso(action.created_at_ms),
    })
}

fn persist_runtime_actions(
    state: &AppState,
    conversation_id: &str,
    task_id: &str,
    actions: &[RuntimeAction],
) -> Result<Vec<PendingActionView>, String> {
    let mut views = Vec::new();
    for action in actions {
        let action_id = action.action_id.to_string();
        let stored = match state
            .storage
            .get_proposed_action(&action_id)
            .map_err(display_error)?
        {
            Some(existing) => existing,
            None => state
                .storage
                .create_proposed_action(&ProposedActionInput {
                    id: action_id.clone(),
                    conversation_id: conversation_id.into(),
                    task_id: Some(task_id.into()),
                    tool_name: action.tool_name.clone(),
                    summary: action.summary.clone(),
                    request: serde_json::to_value(&action.request).map_err(display_error)?,
                })
                .map_err(display_error)?,
        };
        if stored.status == StoredActionStatus::Pending {
            state
                .action_to_task
                .lock()
                .map_err(|_| "Action-map lock was poisoned".to_string())?
                .insert(action_id, task_id.into());
            if let Some(view) = pending_action_from_stored(&stored) {
                views.push(view);
            }
        }
    }
    Ok(views)
}

fn persist_assistant_message(
    storage: &Storage,
    conversation_id: &str,
    task_id: &str,
    message: &ChatMessage,
) -> Result<(), String> {
    let content = message
        .content
        .as_deref()
        .unwrap_or("CrowClaw completed the task without a text response.");
    storage
        .append_message(&MessageInput {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation_id.into(),
            role: StoredMessageRole::Assistant,
            content: content.into(),
            metadata: json!({ "taskId": task_id }),
        })
        .map_err(display_error)?;
    Ok(())
}

fn record_tool_executions(
    storage: &Storage,
    pending: &[PendingToolCall],
    messages: &[ChatMessage],
) -> Result<Vec<(String, MemoryRecord)>, String> {
    let mut recorded = Vec::new();
    for call in pending {
        let Some(message) = messages.iter().find(|message| {
            message.role == ChatRole::Tool
                && message.tool_call_id.as_deref() == Some(&call.provider_tool_call_id)
        }) else {
            continue;
        };
        let action_id = call.proposal.action_id.to_string();
        let validation_error = if message.name.as_deref() != Some(&call.proposal.tool_name) {
            Some(format!(
                "Tool result name did not match approved action: expected {:?}, received {:?}",
                call.proposal.tool_name, message.name
            ))
        } else {
            match message
                .content
                .as_deref()
                .ok_or_else(|| "Tool result had no content".to_string())
                .and_then(|content| {
                    serde_json::from_str::<ToolExecution>(content)
                        .map_err(|error| format!("Tool returned an invalid result: {error}"))
                }) {
                Ok(execution) if execution_action_id(&execution) == &call.proposal.action_id => {
                    let tool_result = serde_json::to_value(&execution).map_err(display_error)?;
                    match execution {
                        ToolExecution::Executed { .. } => {
                            let action = storage
                                .record_action_success(&action_id, &tool_result)
                                .map_err(display_error)?;
                            recorded.push((action_id.clone(), memory_from_action(&action)));
                        }
                        ToolExecution::Failed { error, .. } => {
                            storage
                                .record_action_failure(&action_id, &error.to_string())
                                .map_err(display_error)?;
                        }
                        ToolExecution::Denied { .. } => {
                            // The user's denial was already durably recorded before the
                            // runtime received the decision. Do not rewrite it as success.
                        }
                    }
                    None
                }
                Ok(execution) => Some(format!(
                    "Tool result action ID did not match approved action: expected {action_id}, received {}",
                    execution_action_id(&execution)
                )),
                Err(error) => Some(error),
            }
        };
        if let Some(error) = validation_error {
            if storage
                .get_proposed_action(&action_id)
                .map_err(display_error)?
                .is_some_and(|action| action.status == StoredActionStatus::Approved)
            {
                let action = storage
                    .record_action_failure(&action_id, &error)
                    .map_err(display_error)?;
                debug_assert_eq!(action.status, StoredActionStatus::Failed);
            }
            return Err(error);
        }
    }
    Ok(recorded)
}

fn execution_action_id(execution: &ToolExecution) -> &ActionId {
    match execution {
        ToolExecution::Executed { action_id, .. }
        | ToolExecution::Denied { action_id, .. }
        | ToolExecution::Failed { action_id, .. } => action_id,
    }
}

fn memory_from_action(action: &StoredAction) -> MemoryRecord {
    MemoryRecord {
        id: format!("memory-{}", action.id),
        title: format!("Approved {}", action.tool_name.replace('_', " ")),
        preview: format!(
            "{} — {}",
            action.summary,
            action_target(&action.tool_name, &action.request)
        ),
        source: "approved-action",
        conversation_id: Some(action.conversation_id.clone()),
        created_at: iso(action.updated_at_ms),
        tags: vec!["approved".into(), "local".into(), action.tool_name.clone()],
    }
}

fn crowquant_memory_view(memory: &StoredCrowQuantMemory) -> CrowQuantMemoryView {
    let compressed_bytes = memory.block.len() as u64;
    CrowQuantMemoryView {
        id: memory.id.clone(),
        text: memory.text.clone(),
        created_at: iso(memory.created_at_ms),
        original_bytes: memory.original_bytes,
        compressed_bytes,
        compression_ratio: if compressed_bytes == 0 {
            0.0
        } else {
            memory.original_bytes as f64 / compressed_bytes as f64
        },
        algorithm: memory.algorithm.clone(),
    }
}

fn crowquant_search_hit_view(hit: ServiceCrowQuantSearchHit) -> CrowQuantSearchHit {
    CrowQuantSearchHit {
        memory: crowquant_memory_view(&hit.memory),
        score: hit.score,
    }
}

fn chat_result(
    storage: &Storage,
    conversation_id: &str,
    task_id: &str,
    pending_actions: Vec<PendingActionView>,
) -> Result<ChatTurnResult, StorageError> {
    let conversation = conversation_view(storage, conversation_id)?;
    let task = storage
        .get_task(task_id)?
        .ok_or_else(|| StorageError::InvalidData(format!("task '{task_id}' was not found")))?;
    Ok(ChatTurnResult {
        summary: summary_for(&conversation),
        conversation,
        task: task_view(storage, &task)?,
        pending_actions,
    })
}

fn stored_to_agent_message(message: &Message) -> Option<ChatMessage> {
    match message.role {
        StoredMessageRole::System => Some(ChatMessage::system(message.content.clone())),
        StoredMessageRole::User => Some(ChatMessage::user(message.content.clone())),
        StoredMessageRole::Assistant => Some(ChatMessage::assistant(message.content.clone())),
        StoredMessageRole::Tool => None,
    }
}

fn connection_view(
    profile: &ProviderProfile,
    status: &'static str,
    latency_ms: Option<u64>,
) -> ModelConnection {
    ModelConnection {
        id: profile.id.clone(),
        provider: ProviderKind::from_storage(&profile.provider_kind),
        label: profile.name.clone(),
        base_url: profile.base_url.clone(),
        model: profile.model.clone(),
        status,
        connected_at: Some(iso(profile.updated_at_ms)),
        latency_ms,
    }
}

fn emit_task(app: &tauri::AppHandle, storage: &Storage, task: &StoredTask) -> Result<(), String> {
    app.emit(TASK_EVENT, task_view(storage, task).map_err(display_error)?)
        .map_err(display_error)
}

fn remove_live_task(state: &AppState, task_id: &str) -> Result<(), String> {
    state
        .active_tasks
        .lock()
        .map_err(|_| "Active-task lock was poisoned".to_string())?
        .remove(task_id);
    state
        .action_to_task
        .lock()
        .map_err(|_| "Action-map lock was poisoned".to_string())?
        .retain(|_, owner| owner != task_id);
    Ok(())
}

fn action_kind(tool_name: &str) -> &'static str {
    match tool_name {
        "run_command" => "run-command",
        "remember_memory" | "search_memory" => "memory",
        _ => "read-files",
    }
}

fn action_title(tool_name: &str) -> &'static str {
    match tool_name {
        "list_directory" => "List a selected folder",
        "read_text_file" => "Read a selected text file",
        "run_command" => "Run a local command",
        "remember_memory" => "Remember text with CrowQuant",
        "search_memory" => "Search CrowQuant memory",
        _ => "Run a local action",
    }
}

fn action_target(tool_name: &str, request: &Value) -> String {
    if matches!(tool_name, "remember_memory" | "search_memory") {
        return "CrowClaw local CrowQuant memory".into();
    }
    request
        .get("path")
        .or_else(|| request.get("cwd"))
        .and_then(Value::as_str)
        .or_else(|| request.get("program").and_then(Value::as_str))
        .unwrap_or("Local computer")
        .to_string()
}

fn action_details(tool_name: &str, request: &Value) -> Vec<String> {
    match tool_name {
        "list_directory" => vec![
            "List names and types in the selected folder".into(),
            "Keep access inside the selected folder".into(),
            "Do not read file contents in this action".into(),
        ],
        "read_text_file" => vec![
            format!("Read only {}", action_target(tool_name, request)),
            "Reject binary files and enforce the size boundary".into(),
            "Return the actual approved contents to the local model".into(),
        ],
        "run_command" => vec![
            format!(
                "Run the requested program in {}",
                action_target(tool_name, request)
            ),
            "Capture bounded output".into(),
            "Stop on cancellation or timeout".into(),
        ],
        "remember_memory" => vec![
            format!(
                "Store exactly this text: {:?}",
                request.get("text").and_then(Value::as_str).unwrap_or("")
            ),
            "Create one native CrowQuant compressed lexical record in the local SQLite database"
                .into(),
            "Return the created memory ID and measured compression metadata to the connected model and approved-action audit"
                .into(),
        ],
        "search_memory" => vec![
            format!(
                "Search for exactly: {:?}",
                request.get("query").and_then(Value::as_str).unwrap_or("")
            ),
            format!(
                "Return up to {} top-ranked compressed-lexical results",
                request.get("limit").and_then(Value::as_u64).unwrap_or(5)
            ),
            "Read and return top-ranked stored text with compressed lexical similarity scores to the connected model and approved-action audit"
                .into(),
        ],
        _ => vec!["Run only the action shown here".into()],
    }
}

fn title_from(content: &str) -> String {
    non_empty_or(
        &content.chars().take(54).collect::<String>(),
        "New conversation",
    )
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.into()
    } else {
        value.into()
    }
}

fn iso(milliseconds: i64) -> String {
    Utc.timestamp_millis_opt(milliseconds)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        action_details, action_kind, action_target, action_title, interrupt_unexecuted_action,
        record_tool_executions, AppState,
    };
    use crate::{
        agent::{CancellationToken, ChatMessage, PendingToolCall},
        crowquant_memory::{agent_memory_id, CrowQuantMemoryService},
        storage::{ActionStatus, ConversationInput, ProposedActionInput, Storage},
        tools::{
            MemorySearchMatch, RememberedMemory, ToolExecution, ToolExecutor, ToolOutput,
            ToolPolicy, ToolRequest,
        },
    };

    #[test]
    fn memory_approval_copy_is_explicit_about_text_query_limit_and_exposure() {
        let remember = json!({
            "type": "remember_memory",
            "text": "exact memory text"
        });
        assert_eq!(action_kind("remember_memory"), "memory");
        assert_eq!(
            action_title("remember_memory"),
            "Remember text with CrowQuant"
        );
        assert_eq!(
            action_target("remember_memory", &remember),
            "CrowClaw local CrowQuant memory"
        );
        assert!(action_details("remember_memory", &remember)
            .join(" ")
            .contains("exact memory text"));

        let search = json!({
            "type": "search_memory",
            "query": "exact search query",
            "limit": 7
        });
        let details = action_details("search_memory", &search).join(" ");
        assert!(details.contains("exact search query"));
        assert!(details.contains('7'));
        assert!(details.contains("connected model and approved-action audit"));
    }

    #[test]
    fn batched_tool_results_are_audited_by_exact_provider_call_id() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(directory.path()).unwrap();
        storage
            .create_conversation(&ConversationInput {
                id: "conversation-1".into(),
                title: "Memory batch".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let executor = ToolExecutor::new(ToolPolicy::default()).unwrap();
        let remember = executor
            .propose(ToolRequest::RememberMemory {
                text: "qubit calibration".into(),
            })
            .unwrap();
        let search = executor
            .propose(ToolRequest::SearchMemory {
                query: "qubit".into(),
                limit: 3,
            })
            .unwrap();
        for action in [&remember, &search] {
            storage
                .create_proposed_action(&ProposedActionInput {
                    id: action.action_id.to_string(),
                    conversation_id: "conversation-1".into(),
                    task_id: None,
                    tool_name: action.tool_name.clone(),
                    summary: action.summary.clone(),
                    request: serde_json::to_value(&action.request).unwrap(),
                })
                .unwrap();
            storage
                .approve_action(&action.action_id.to_string(), Some("test approval"))
                .unwrap();
        }
        let pending = vec![
            PendingToolCall {
                provider_tool_call_id: "call-remember".into(),
                proposal: remember.clone(),
            },
            PendingToolCall {
                provider_tool_call_id: "call-search".into(),
                proposal: search.clone(),
            },
        ];
        let remember_result = ToolExecution::Executed {
            action_id: remember.action_id.clone(),
            output: ToolOutput::MemoryRemembered {
                memory: RememberedMemory {
                    id: "remembered-row".into(),
                    text: "qubit calibration".into(),
                    created_at_ms: 1,
                    original_bytes: 2048,
                    compressed_bytes: 161,
                    algorithm: "CrowQuant test".into(),
                },
            },
        };
        let search_result = ToolExecution::Executed {
            action_id: search.action_id.clone(),
            output: ToolOutput::MemorySearch {
                query: "qubit".into(),
                results: vec![MemorySearchMatch {
                    id: "searched-row".into(),
                    text: "stored qubit record".into(),
                    created_at_ms: 1,
                    score: 0.8,
                }],
            },
        };
        // Deliberately reverse the result order: newest-message guessing would
        // bind these to the wrong stored actions.
        let messages = vec![
            ChatMessage::tool(
                "call-search",
                "search_memory",
                serde_json::to_string(&search_result).unwrap(),
            ),
            ChatMessage::tool(
                "call-remember",
                "remember_memory",
                serde_json::to_string(&remember_result).unwrap(),
            ),
        ];

        let recorded = record_tool_executions(&storage, &pending, &messages).unwrap();
        assert_eq!(recorded.len(), 2);
        let stored_remember = storage
            .get_proposed_action(&remember.action_id.to_string())
            .unwrap()
            .unwrap();
        let stored_search = storage
            .get_proposed_action(&search.action_id.to_string())
            .unwrap()
            .unwrap();
        assert_eq!(stored_remember.status, ActionStatus::Succeeded);
        assert_eq!(stored_search.status, ActionStatus::Succeeded);
        assert_eq!(
            stored_remember
                .result
                .as_ref()
                .and_then(|value| value.pointer("/output/memory/id"))
                .and_then(|value| value.as_str()),
            Some("remembered-row")
        );
        assert_eq!(
            stored_search
                .result
                .as_ref()
                .and_then(|value| value.pointer("/output/results/0/id"))
                .and_then(|value| value.as_str()),
            Some("searched-row")
        );
    }

    #[test]
    fn mismatched_embedded_action_id_is_failed_instead_of_misaudited() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(directory.path()).unwrap();
        storage
            .create_conversation(&ConversationInput {
                id: "conversation-1".into(),
                title: "Memory mismatch".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let executor = ToolExecutor::new(ToolPolicy::default()).unwrap();
        let expected = executor
            .propose(ToolRequest::SearchMemory {
                query: "qubit".into(),
                limit: 3,
            })
            .unwrap();
        let wrong = executor
            .propose(ToolRequest::SearchMemory {
                query: "grocery".into(),
                limit: 3,
            })
            .unwrap();
        storage
            .create_proposed_action(&ProposedActionInput {
                id: expected.action_id.to_string(),
                conversation_id: "conversation-1".into(),
                task_id: None,
                tool_name: expected.tool_name.clone(),
                summary: expected.summary.clone(),
                request: serde_json::to_value(&expected.request).unwrap(),
            })
            .unwrap();
        storage
            .approve_action(&expected.action_id.to_string(), Some("test approval"))
            .unwrap();
        let pending = vec![PendingToolCall {
            provider_tool_call_id: "call-search".into(),
            proposal: expected.clone(),
        }];
        let mismatched = ToolExecution::Executed {
            action_id: wrong.action_id,
            output: ToolOutput::MemorySearch {
                query: "grocery".into(),
                results: Vec::new(),
            },
        };
        let messages = vec![ChatMessage::tool(
            "call-search",
            "search_memory",
            serde_json::to_string(&mismatched).unwrap(),
        )];

        let error = record_tool_executions(&storage, &pending, &messages).unwrap_err();
        assert!(error.contains("action ID did not match"));
        let stored = storage
            .get_proposed_action(&expected.action_id.to_string())
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, ActionStatus::Failed);
        assert!(stored
            .error
            .as_deref()
            .is_some_and(|value| value.contains("action ID did not match")));
    }

    #[test]
    fn cancellation_closes_approved_and_pending_batch_actions() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(directory.path()).unwrap();
        storage
            .create_conversation(&ConversationInput {
                id: "conversation-1".into(),
                title: "Cancelled batch".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let executor = ToolExecutor::new(ToolPolicy::default()).unwrap();
        let approved = executor
            .propose(ToolRequest::RememberMemory {
                text: "approved but not executed".into(),
            })
            .unwrap();
        let pending = executor
            .propose(ToolRequest::SearchMemory {
                query: "still pending".into(),
                limit: 3,
            })
            .unwrap();
        for action in [&approved, &pending] {
            storage
                .create_proposed_action(&ProposedActionInput {
                    id: action.action_id.to_string(),
                    conversation_id: "conversation-1".into(),
                    task_id: None,
                    tool_name: action.tool_name.clone(),
                    summary: action.summary.clone(),
                    request: serde_json::to_value(&action.request).unwrap(),
                })
                .unwrap();
        }
        storage
            .approve_action(&approved.action_id.to_string(), Some("approved once"))
            .unwrap();

        for action in [&approved, &pending] {
            interrupt_unexecuted_action(
                &storage,
                &action.action_id.to_string(),
                "Task cancelled by user",
            )
            .unwrap();
        }

        assert_eq!(
            storage
                .get_proposed_action(&approved.action_id.to_string())
                .unwrap()
                .unwrap()
                .status,
            ActionStatus::Failed
        );
        assert_eq!(
            storage
                .get_proposed_action(&pending.action_id.to_string())
                .unwrap()
                .unwrap()
                .status,
            ActionStatus::Denied
        );
    }

    #[test]
    fn restart_closes_approved_and_pending_batch_actions() {
        let directory = tempdir().unwrap();
        let storage = Storage::open(directory.path()).unwrap();
        storage
            .create_conversation(&ConversationInput {
                id: "conversation-1".into(),
                title: "Restarted batch".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let executor = ToolExecutor::new(ToolPolicy::default()).unwrap();
        let approved = executor
            .propose(ToolRequest::SearchMemory {
                query: "approved but not executed".into(),
                limit: 3,
            })
            .unwrap();
        let pending = executor
            .propose(ToolRequest::RememberMemory {
                text: "still pending".into(),
            })
            .unwrap();
        for action in [&approved, &pending] {
            storage
                .create_proposed_action(&ProposedActionInput {
                    id: action.action_id.to_string(),
                    conversation_id: "conversation-1".into(),
                    task_id: None,
                    tool_name: action.tool_name.clone(),
                    summary: action.summary.clone(),
                    request: serde_json::to_value(&action.request).unwrap(),
                })
                .unwrap();
        }
        storage
            .approve_action(&approved.action_id.to_string(), Some("approved once"))
            .unwrap();
        drop(storage);

        let reopened = AppState::open(directory.path().to_path_buf()).unwrap();
        assert_eq!(
            reopened
                .storage
                .get_proposed_action(&approved.action_id.to_string())
                .unwrap()
                .unwrap()
                .status,
            ActionStatus::Failed
        );
        assert_eq!(
            reopened
                .storage
                .get_proposed_action(&pending.action_id.to_string())
                .unwrap()
                .unwrap()
                .status,
            ActionStatus::Denied
        );
    }

    #[test]
    fn restart_recovers_action_audit_after_durable_remember_insert() {
        let directory = tempdir().unwrap();
        let storage = Arc::new(Storage::open(directory.path()).unwrap());
        storage
            .create_conversation(&ConversationInput {
                id: "conversation-1".into(),
                title: "Recovered memory".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let executor = ToolExecutor::new(ToolPolicy::default()).unwrap();
        let action = executor
            .propose(ToolRequest::RememberMemory {
                text: "durable before audit".into(),
            })
            .unwrap();
        storage
            .create_proposed_action(&ProposedActionInput {
                id: action.action_id.to_string(),
                conversation_id: "conversation-1".into(),
                task_id: None,
                tool_name: action.tool_name.clone(),
                summary: action.summary.clone(),
                request: serde_json::to_value(&action.request).unwrap(),
            })
            .unwrap();
        storage
            .approve_action(&action.action_id.to_string(), Some("approved once"))
            .unwrap();
        let service = CrowQuantMemoryService::new(storage.clone());
        let inserted = service
            .remember_agent_record(
                &action.action_id,
                "durable before audit",
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(inserted.id, agent_memory_id(&action.action_id));
        drop(service);
        drop(storage);

        let reopened = AppState::open(directory.path().to_path_buf()).unwrap();
        let stored = reopened
            .storage
            .get_proposed_action(&action.action_id.to_string())
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, ActionStatus::Succeeded);
        assert_eq!(
            stored
                .result
                .as_ref()
                .and_then(|value| value.pointer("/output/memory/id"))
                .and_then(|value| value.as_str()),
            Some(agent_memory_id(&action.action_id).as_str())
        );
        assert_eq!(reopened.storage.list_crowquant_memories().unwrap().len(), 1);
    }
}
