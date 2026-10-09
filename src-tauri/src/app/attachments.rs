//! User-selected file snapshots. This does not grant tool access to a directory.
use super::*;
use crate::agent::AttachmentContent;
use crate::storage::attachments::{AttachmentInput, AttachmentKind, AttachmentSummary};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = 40 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_FILES: usize = 8;

pub(super) fn content(
    record: crate::storage::attachments::AttachmentRecord,
) -> Result<AttachmentContent, String> {
    validate_record(&record)?;
    let name = record.summary.name;
    Ok(match record.summary.kind {
        AttachmentKind::Text => AttachmentContent::Text {
            name,
            text: String::from_utf8(record.bytes)
                .map_err(|_| "Stored attachment is not valid UTF-8".to_string())?,
        },
        AttachmentKind::Image => AttachmentContent::Image {
            name,
            media_type: record.summary.media_type,
            data_base64: STANDARD.encode(record.bytes),
        },
        AttachmentKind::File => AttachmentContent::File {
            name,
            media_type: record.summary.media_type,
            data_base64: STANDARD.encode(record.bytes),
        },
    })
}

fn validate_record(record: &crate::storage::attachments::AttachmentRecord) -> Result<(), String> {
    if record.bytes.is_empty()
        || record.bytes.len() as u64 > MAX_FILE_BYTES
        || record.bytes.len() as u64 != record.summary.byte_length
        || format!("{:x}", Sha256::digest(&record.bytes)) != record.summary.sha256
    {
        return Err(
            "The retained attachment failed its integrity check; no content was sent".into(),
        );
    }
    let (kind, mime) = classify(&record.summary.name, &record.bytes)?;
    if kind != record.summary.kind || mime != record.summary.media_type {
        return Err("The retained attachment type does not match its contents".into());
    }
    Ok(())
}

fn decoded_size(encoded: &str) -> usize {
    (encoded.len() / 4 * 3).saturating_sub(
        encoded
            .as_bytes()
            .iter()
            .rev()
            .take_while(|byte| **byte == b'=')
            .count(),
    )
}

pub(super) async fn validate_support(
    state: &AppState,
    profile: &ProviderProfile,
    messages: &[ChatMessage],
) -> Result<(), String> {
    let attachments: Vec<_> = messages.iter().flat_map(|m| m.attachments.iter()).collect();
    if attachments.is_empty() {
        return Ok(());
    }
    if attachments.len() > MAX_FILES {
        return Err("This conversation exceeds eight attachments in a model request; start a new conversation for more files".into());
    }
    if profile.provider_kind == "crowbot-ai" {
        let mut images = 0;
        for attachment in &attachments {
            match attachment {
                AttachmentContent::Image{media_type,data_base64,..} => {
                    images+=1;
                    if images>2 || !matches!(media_type.as_str(),"image/png"|"image/jpeg"|"image/webp") || decoded_size(data_base64)>10*1024*1024 {
                        return Err("CrowBot picture input supports at most two PNG/JPEG/WebP images of 10 MiB each; originals and the draft are retained".into());
                    }
                }
                AttachmentContent::File{..} => return Err("CrowBot's chat picture route requires extracted text or PNG/JPEG/WebP input; raw documents are retained but not sent".into()),
                AttachmentContent::Text{..} => {}
            }
        }
    }
    let mut total = 0usize;
    let mut needs_vision = false;
    let mut needs_files = false;
    for item in attachments {
        let size = match item {
            AttachmentContent::Text { text, .. } => text.len(),
            AttachmentContent::Image { data_base64, .. } => {
                needs_vision = true;
                decoded_size(data_base64)
            }
            AttachmentContent::File {
                data_base64,
                media_type,
                ..
            } => {
                needs_files = true;
                needs_vision |= media_type == "application/pdf";
                decoded_size(data_base64)
            }
        };
        total = total.checked_add(size).ok_or("Attachment size overflow")?;
    }
    if total > MAX_BATCH_BYTES {
        return Err("Attachments in this conversation exceed the 40 MiB request limit; start a new conversation".into());
    }
    if !needs_vision && !needs_files {
        return Ok(());
    }
    if profile.provider_kind == "openrouter" {
        if needs_files {
            return Err("Raw document parsing is not enabled for OpenRouter free-only requests. Attach extracted text or choose a supported document provider; no paid parsing plugin was used".into());
        }
        let model = super::openrouter::cached_model(state, profile, None)?;
        return if !needs_vision || model.input_modalities.iter().any(|mode| mode == "image") {
            Ok(())
        } else {
            Err("This OpenRouter free model does not advertise image input. Choose a free vision model or remove the image".into())
        };
    }
    if profile.provider_kind == "chatgpt" {
        // Documented model modalities, not a guess from naming. Account availability
        // is separately validated by composer::profile_for_choice. New catalog
        // models remain unknown until their modality contract has been verified.
        if matches!(profile.model.as_str(), "gpt-6-luna" | "gpt-6.1-sol") {
            return Ok(());
        }
        return Err("Image/document support has not been verified for this account model. Choose a supported model or remove the attachment; no fallback was used".into());
    }
    if profile.provider_kind == "crowbot-ai" {
        return Ok(());
    }
    if needs_files {
        return Err("This local provider cannot receive raw documents yet. Attach UTF-8 text or use a supported document model; no file was sent".into());
    }
    if local_vision(state, profile).await? {
        Ok(())
    } else {
        Err("The selected model does not advertise image support. Choose a vision model or remove the image; no file was sent".into())
    }
}

async fn local_vision(state: &AppState, profile: &ProviderProfile) -> Result<bool, String> {
    let config = config_from_profile(state, profile)?;
    let mut endpoint =
        reqwest::Url::parse(&config.base_url).map_err(|_| "Invalid model endpoint")?;
    if endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err("Model endpoint contains unsupported URL credentials or parameters".into());
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "Could not create capability client")?;
    let mut request=match profile.provider_kind.as_str() {
        "lm-studio" => {endpoint.set_path("/api/v1/models");client.get(endpoint)},
        "ollama" => {endpoint.set_path("/api/show");client.post(endpoint).json(&json!({"model":profile.model}))},
        _ => return Err("This compatible endpoint has no verified model-capability adapter; image sending is unavailable".into()),
    };
    if let Some(key) = config.api_key {
        request = request.bearer_auth(key);
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| "Could not verify this model's image capabilities; no file was sent")?;
    if !response.status().is_success() {
        return Err(
            "The model server could not confirm image capabilities; no file was sent".into(),
        );
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Model capability response was interrupted")?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("Model capability response exceeded its limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let catalog: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "Model server returned invalid capability metadata")?;
    Ok(catalog_has_vision(
        &profile.provider_kind,
        &profile.model,
        &catalog,
    ))
}

fn catalog_has_vision(provider: &str, model: &str, catalog: &Value) -> bool {
    if provider == "ollama" {
        return catalog
            .get("capabilities")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|v| v.as_str() == Some("vision")));
    }
    if provider != "lm-studio" {
        return false;
    }
    catalog
        .get("models")
        .and_then(Value::as_array)
        .and_then(|models| {
            models.iter().find(|m| {
                m.get("type").and_then(Value::as_str) == Some("llm")
                    && (m.get("key").and_then(Value::as_str) == Some(model)
                        || m.get("loaded_instances")
                            .and_then(Value::as_array)
                            .is_some_and(|instances| {
                                instances
                                    .iter()
                                    .any(|i| i.get("id").and_then(Value::as_str) == Some(model))
                            }))
            })
        })
        .and_then(|m| m.get("capabilities"))
        .and_then(|c| c.get("vision"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectRequest {
    conversation_id: String,
    revision: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveRequest {
    conversation_id: String,
    revision: u32,
    attachment_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewRequest {
    conversation_id: String,
    attachment_id: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPreview {
    attachment: AttachmentSummary,
    text: Option<String>,
    data_url: Option<String>,
}

#[tauri::command]
pub async fn crowclaw_attachments_select(
    state: State<'_, AppState>,
    request: SelectRequest,
) -> Result<composer::ComposerSnapshot, String> {
    let current = composer::ensure(&state, &request.conversation_id)?;
    if current.revision != request.revision {
        return Err("This chat changed; refresh its composer before attaching files".into());
    }
    let inputs = tauri::async_runtime::spawn_blocking(|| {
        let selected = rfd::FileDialog::new()
            .set_title("Attach files to this conversation")
            .pick_files();
        selected.map(read_selection).transpose()
    })
    .await
    .map_err(|_| "The file picker could not finish".to_string())??;
    let saved = match inputs {
        Some(inputs) => state
            .storage
            .add_composer_attachments(&request.conversation_id, request.revision, &inputs)
            .map_err(display_error)?,
        None => unchanged_after_cancel(&state.storage, &request)?,
    };
    composer::snapshot(&state, saved)
}

fn unchanged_after_cancel(
    storage: &Storage,
    request: &SelectRequest,
) -> Result<crate::storage::ConversationComposer, String> {
    let current = storage
        .conversation_composer(&request.conversation_id)
        .map_err(display_error)?;
    if current.revision != request.revision {
        return Err("This conversation changed while selecting files. Refresh the composer to resolve the conflict".into());
    }
    Ok(current)
}

#[tauri::command]
pub fn crowclaw_attachment_remove(
    state: State<'_, AppState>,
    request: RemoveRequest,
) -> Result<composer::ComposerSnapshot, String> {
    let saved = state
        .storage
        .remove_composer_attachment(
            &request.conversation_id,
            request.revision,
            &request.attachment_id,
        )
        .map_err(display_error)?;
    composer::snapshot(&state, saved)
}

#[tauri::command]
pub fn crowclaw_attachment_preview(
    state: State<'_, AppState>,
    request: PreviewRequest,
) -> Result<AttachmentPreview, String> {
    let record = state
        .storage
        .attachment(&request.conversation_id, &request.attachment_id)
        .map_err(display_error)?;
    validate_record(&record)?;
    let text = if record.summary.kind == AttachmentKind::Text {
        Some(
            String::from_utf8(record.bytes.clone())
                .map_err(|_| "Stored text attachment is invalid".to_string())?,
        )
    } else {
        None
    };
    let data_url = if record.summary.kind == AttachmentKind::Image
        || record.summary.media_type == "application/pdf"
    {
        Some(format!(
            "data:{};base64,{}",
            record.summary.media_type,
            STANDARD.encode(&record.bytes)
        ))
    } else {
        None
    };
    Ok(AttachmentPreview {
        attachment: record.summary,
        text,
        data_url,
    })
}

fn read_selection(paths: Vec<PathBuf>) -> Result<Vec<AttachmentInput>, String> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err("Choose between one and eight files".into());
    }
    let mut total = 0usize;
    let mut inputs = Vec::with_capacity(paths.len());
    for path in paths {
        let input = read_selected_file(&path)?;
        total = total
            .checked_add(input.bytes.len())
            .ok_or("Attachment size overflow")?;
        if total > MAX_BATCH_BYTES {
            return Err("Selected files exceed the 40 MiB combined limit".into());
        }
        inputs.push(input);
    }
    Ok(inputs)
}

fn read_selected_file(path: &Path) -> Result<AttachmentInput, String> {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("This file has an unsupported name")?
        .to_string();
    if name.is_empty()
        || name.len() > 255
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    {
        return Err("Attachment names must not contain paths or control characters".into());
    }
    let mut file =
        std::fs::File::open(path).map_err(|_| "Could not open the selected file".to_string())?;
    let before = file
        .metadata()
        .map_err(|_| "Could not inspect the selected file".to_string())?;
    if !before.is_file() || before.len() == 0 || before.len() > MAX_FILE_BYTES {
        return Err("Choose a nonempty regular file no larger than 20 MiB".into());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    (&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the selected file".to_string())?;
    let after = file
        .metadata()
        .map_err(|_| "Could not recheck the selected file".to_string())?;
    if bytes.len() as u64 != before.len()
        || after.len() != before.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("The file changed while being attached; select it again".into());
    }
    let (kind, media_type) = classify(&name, &bytes)?;
    Ok(AttachmentInput {
        id: Uuid::new_v4().to_string(),
        name,
        media_type: media_type.into(),
        kind,
        bytes,
    })
}

fn classify(name: &str, bytes: &[u8]) -> Result<(AttachmentKind, &'static str), String> {
    let extension = Path::new(name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let image = match extension.as_str() {
        "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Some("image/png"),
        "jpg" | "jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Some("image/jpeg"),
        "webp" if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => {
            Some("image/webp")
        }
        "gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => Some("image/gif"),
        _ => None,
    };
    if let Some(mime) = image {
        return Ok((AttachmentKind::Image, mime));
    }
    if extension == "pdf" && bytes.starts_with(b"%PDF-") {
        return Ok((AttachmentKind::File, "application/pdf"));
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let mime = match extension.as_str() {
            "docx" => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
            }
            "pptx" => {
                Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
            }
            "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
            _ => None,
        };
        if let Some(mime) = mime {
            return Ok((AttachmentKind::File, mime));
        }
    }
    if matches!(
        extension.as_str(),
        "txt"
            | "md"
            | "markdown"
            | "json"
            | "csv"
            | "tsv"
            | "log"
            | "rs"
            | "py"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "css"
            | "html"
            | "htm"
            | "xml"
            | "yaml"
            | "yml"
            | "toml"
            | "ini"
            | "cfg"
            | "sh"
            | "ps1"
            | "c"
            | "h"
            | "cpp"
            | "java"
            | "sql"
    ) {
        if bytes.len() > MAX_TEXT_BYTES || bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
        {
            return Err(
                "Text attachments must be UTF-8, contain no NUL bytes and be no larger than 1 MiB"
                    .into(),
            );
        }
        return Ok((
            AttachmentKind::Text,
            match extension.as_str() {
                "md" | "markdown" => "text/markdown",
                "json" => "application/json",
                _ => "text/plain",
            },
        ));
    }
    Err("Unsupported file type or mismatched file header. Choose UTF-8 text/code, PNG, JPEG, WebP, GIF, PDF, DOCX, PPTX or XLSX".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_content_is_read_without_retaining_source_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "saved text").unwrap();
        let captured = read_selected_file(&path).unwrap();
        std::fs::write(&path, "changed after selection").unwrap();
        assert_eq!(captured.bytes, b"saved text");
        assert_eq!(captured.name, "note.txt");
        assert_eq!(captured.kind, AttachmentKind::Text);
    }
    #[test]
    fn directories_binary_text_and_disguised_images_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_selected_file(dir.path()).is_err());
        assert!(classify("photo.png", b"not an image").is_err());
        assert!(classify("secret.txt", b"binary\0text").is_err());
        assert!(classify("script.svg", b"<svg onload='run()'/>").is_err());
        assert!(classify("archive.zip", b"PK\x03\x04").is_err());
    }
    #[test]
    fn text_is_display_data_not_rendered_markup() {
        assert_eq!(
            classify("page.html", b"<script>alert(1)</script>").unwrap(),
            (AttachmentKind::Text, "text/plain")
        );
        assert!(classify("large.txt", &vec![b'a'; MAX_TEXT_BYTES + 1]).is_err());
    }

    #[test]
    fn capabilities_belong_to_selected_model_not_another_catalog_entry() {
        let catalog = json!({"models":[
            {"type":"llm","key":"vision-key","loaded_instances":[{"id":"chosen-instance"}],"capabilities":{"vision":true}},
            {"type":"llm","key":"text-only","capabilities":{"vision":false}},
            {"type":"embedding","key":"embedding","capabilities":{"vision":true}}
        ]});
        assert!(catalog_has_vision("lm-studio", "chosen-instance", &catalog));
        assert!(catalog_has_vision("lm-studio", "vision-key", &catalog));
        for model in ["text-only", "unknown", "embedding"] {
            assert!(!catalog_has_vision("lm-studio", model, &catalog));
        }
        assert!(!catalog_has_vision(
            "compatible",
            "chosen-instance",
            &catalog
        ));
        assert!(catalog_has_vision(
            "ollama",
            "selected",
            &json!({"capabilities":["completion","vision"]})
        ));
        assert!(!catalog_has_vision(
            "ollama",
            "selected",
            &json!({"capabilities":["completion"]})
        ));
    }

    #[test]
    fn retained_content_integrity_is_checked_before_preview_or_send() {
        let bytes = b"retained text".to_vec();
        let mut record = crate::storage::attachments::AttachmentRecord {
            summary: AttachmentSummary {
                id: "attachment".into(),
                conversation_id: "chat".into(),
                message_id: None,
                name: "note.txt".into(),
                media_type: "text/plain".into(),
                kind: AttachmentKind::Text,
                byte_length: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                created_at_ms: 1,
            },
            bytes,
        };
        assert!(validate_record(&record).is_ok());
        record.bytes[0] = b'X';
        assert!(validate_record(&record).is_err());
        record.summary.sha256 = format!("{:x}", Sha256::digest(&record.bytes));
        record.summary.media_type = "image/png".into();
        assert!(validate_record(&record).is_err());
    }

    #[test]
    fn base64_padding_does_not_inflate_request_limits() {
        for size in [1, 2, 3, 4, 10] {
            assert_eq!(decoded_size(&STANDARD.encode(vec![0; size])), size);
        }
    }

    #[test]
    fn cancelling_picker_does_not_adopt_another_windows_revision() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        storage
            .create_conversation(&ConversationInput {
                id: "chat".into(),
                title: "Chat".into(),
                provider_profile_id: None,
            })
            .unwrap();
        let request = SelectRequest {
            conversation_id: "chat".into(),
            revision: 0,
        };
        assert_eq!(
            unchanged_after_cancel(&storage, &request).unwrap().revision,
            0
        );
        storage
            .save_conversation_composer("chat", 0, "another window", None)
            .unwrap();
        assert!(unchanged_after_cancel(&storage, &request).is_err());
        assert_eq!(
            storage.conversation_composer("chat").unwrap().draft,
            "another window"
        );
    }
}
