//! CrowClaw-owned anonymous image transport. No donor app, paid key or printer.
use crate::{
    agent::{CancellationToken, ToolDefinition},
    tools::{GeneratedImage, ImageGenerationBackend, ToolError},
};
use async_trait::async_trait;
use md5::{Digest, Md5};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use std::time::Duration;

const LIMIT: usize = 12 * 1024 * 1024;
pub struct CrowBotImageGenerator;
impl CrowBotImageGenerator {
    pub fn new() -> Self {
        Self
    }
}
fn invalid(message: &str) -> ToolError {
    ToolError::InvalidRequest {
        tool_name: "generate_image".into(),
        message: message.into(),
    }
}
fn envelope(timestamp: u64, prompt: &str) -> Value {
    // Public shipped protocol checksum; no user credential or entitlement.
    let authorization = format!(
        "{:x}",
        Md5::digest(format!("{timestamp}.nice@friend.ai.0.{timestamp}"))
    );
    json!({"Authorization":authorization,"app_version":"8.07.29","app_name":"FriendAI","user_id":0,"timestamp":timestamp,"prompt":prompt,"style":""})
}
fn media_url(raw: &str) -> Result<Url, ToolError> {
    let url =
        Url::parse(raw).map_err(|_| invalid("CrowBot AI returned an invalid image address"))?;
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !["ocrmath.com", "yintb.com", "xfyun.cn", "friendai.cloud"]
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        return Err(invalid(
            "CrowBot AI image address left the known HTTPS media services",
        ));
    }
    Ok(url)
}
async fn body(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ToolError> {
    let mut response = response;
    if response.status().as_u16() != 200 || response.headers().contains_key("content-range") {
        return Err(invalid(&format!(
            "CrowBot AI image service returned HTTP {}; not retried",
            response.status().as_u16()
        )));
    }
    let length = response.content_length();
    if length.is_some_and(|n| n > limit as u64) {
        return Err(invalid("CrowBot AI image response exceeded its byte bound"));
    }
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| invalid("CrowBot AI image response was interrupted; not retried"))?
    {
        if data.len().saturating_add(chunk.len()) > limit {
            return Err(invalid("CrowBot AI image response exceeded its byte bound"));
        }
        data.extend_from_slice(&chunk);
    }
    if data.is_empty() || length.is_some_and(|n| n != data.len() as u64) {
        return Err(invalid(
            "CrowBot AI image response was incomplete; not retried",
        ));
    }
    Ok(data)
}
fn image_type(bytes: &[u8]) -> Result<(&'static str, &'static str), ToolError> {
    if bytes.len() < 12 || bytes.len() > LIMIT {
        return Err(invalid("CrowBot AI returned an empty or oversized image"));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(("image/png", "png"));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.ends_with(&[0xff, 0xd9]) {
        return Ok(("image/jpeg", "jpg"));
    }
    if &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(("image/webp", "webp"));
    }
    Err(invalid(
        "CrowBot AI returned no supported PNG, JPEG or WebP image",
    ))
}

#[async_trait]
impl ImageGenerationBackend for CrowBotImageGenerator {
    fn tool_definition(&self) -> ToolDefinition {
        ToolDefinition{name:"generate_image".into(),description:"Create one real image when requested with CrowBot AI's direct anonymous internet service. No ChatGPT membership or paid API key is used. It runs directly unless the owner explicitly enables optional image confirmation. This service chooses output dimensions and does not accept OpenAI quality/size controls. This operation never prints.".into(),
            parameters:json!({"type":"object","properties":{"prompt":{"type":"string","minLength":1,"maxLength":16384}},"required":["prompt"],"additionalProperties":false})}
    }
    async fn generate(
        &self,
        prompt: &str,
        _quality: &str,
        _size: &str,
        cancellation: &CancellationToken,
    ) -> Result<GeneratedImage, ToolError> {
        if prompt.trim().is_empty() || prompt.len() > 16384 {
            return Err(invalid("Image prompt must contain at most 16 KiB of text"));
        }
        let operation = async {
            let client = Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90))
                .build()
                .map_err(|_| invalid("Could not create CrowBot AI image transport"))?;
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| invalid("Invalid system clock"))?
                .as_millis();
            let payload = serde_json::to_vec(&envelope(
                u64::try_from(timestamp).map_err(|_| invalid("Invalid system clock"))?,
                prompt,
            ))
            .map_err(|_| invalid("Could not serialize image request"))?;
            if payload.len() > 1024 * 1024 {
                return Err(invalid(
                    "CrowBot AI image request exceeded its complete byte bound",
                ));
            }
            let response = client
                .post("https://miaoxue.api.open.ocrmath.com/picture/pureTextToPicture")
                .header("Content-Type", "application/json")
                .header("Accept-Language", "en")
                .header("device_id", "")
                .body(payload)
                .send()
                .await
                .map_err(|_| invalid("CrowBot AI image request failed; not retried"))?;
            let metadata: Value = serde_json::from_slice(&body(response, 1024 * 1024).await?)
                .map_err(|_| invalid("CrowBot AI returned invalid image metadata"))?;
            let urls = metadata["data"].as_array().filter(|urls| urls.len() == 1);
            if metadata["errno"] != 0 || urls.is_none() {
                return Err(invalid(
                    "CrowBot AI did not return exactly one image; not retried",
                ));
            }
            let mut url = media_url(
                urls.unwrap()[0]
                    .as_str()
                    .ok_or_else(|| invalid("CrowBot AI returned invalid image metadata"))?,
            )?;
            let mut downloaded = None;
            for hop in 0..=3 {
                let response = client
                    .get(url.clone())
                    .send()
                    .await
                    .map_err(|_| invalid("CrowBot AI image download failed; not retried"))?;
                if response.status().is_redirection() {
                    if hop == 3 {
                        return Err(invalid("CrowBot AI image exceeded its redirect bound"));
                    }
                    let location = response
                        .headers()
                        .get("location")
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(|| invalid("Image redirect had no valid location"))?;
                    url = media_url(
                        url.join(location)
                            .map_err(|_| invalid("Invalid image redirect"))?
                            .as_str(),
                    )?;
                } else {
                    downloaded = Some(body(response, LIMIT).await?);
                    break;
                }
            }
            let bytes = downloaded.ok_or_else(|| invalid("CrowBot AI image was not downloaded"))?;
            let (media_type, extension) = image_type(&bytes)?;
            Ok(GeneratedImage {
                id: uuid::Uuid::new_v4().to_string(),
                name: format!(
                    "crowclaw-crowbot-{}.{}",
                    uuid::Uuid::new_v4().simple(),
                    extension
                ),
                media_type: media_type.into(),
                model: super::CROWBOT_MODEL.into(),
                bytes,
            })
        };
        tokio::select! { biased; _=cancellation.cancelled()=>Err(ToolError::Cancelled), result=tokio::time::timeout(Duration::from_secs(90),operation)=>result.map_err(|_|invalid("CrowBot AI image deadline exceeded; not retried"))? }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checksum_matches_two_original_wire_vectors() {
        assert_eq!(
            envelope(1700000000000, "cat")["Authorization"],
            "3c413cc14c1f09da35c007a723d05cd2"
        );
        assert_eq!(
            envelope(1757900000123, "cat")["Authorization"],
            "2e3a2ca5900c4cf9a0ccea5bab794094"
        );
        let input = "Complete café 🐦\nimage prompt";
        assert_eq!(envelope(0, input)["prompt"], input);
        assert_eq!(envelope(0, input)["user_id"], 0);
    }
    #[test]
    fn rejects_untrusted_media_and_exposes_only_actual_controls() {
        for url in [
            "http://media.ocrmath.com/image.jpg",
            "https://ocrmath.com.evil.example/image.jpg",
            "https://127.0.0.1/image.jpg",
            "https://user:password@media.ocrmath.com/image.jpg",
        ] {
            assert!(media_url(url).is_err());
        }
        assert!(media_url("https://media.ocrmath.com:9000/image.jpg").is_ok());
        assert!(image_type(b"not an image response").is_err());
        let definition = CrowBotImageGenerator::new().tool_definition();
        assert!(definition.parameters["properties"].get("quality").is_none());
        assert!(definition.parameters["properties"].get("size").is_none());
    }

    #[tokio::test]
    #[ignore = "Explicit authorized one-image direct service test; no paid key or printer"]
    async fn authorized_direct_image_probe() {
        assert_eq!(
            std::env::var("CROWCLAW_CROWBOT_IMAGE_PROBE_ALLOW").as_deref(),
            Ok("1")
        );
        let image = CrowBotImageGenerator::new()
            .generate(
                "A simple illustration of a cat. No text.",
                "medium",
                "1536x1024",
                &CancellationToken::new(),
            )
            .await
            .expect("Image service failed; do not retry automatically");
        assert!(!image.bytes.is_empty());
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".test-runtime/crowbot-image-probe");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(&image.name), &image.bytes).unwrap();
        println!(
            "Actual direct image returned: type={}, bytes={}, output={}",
            image.media_type,
            image.bytes.len(),
            folder.join(&image.name).display()
        );
    }
}
