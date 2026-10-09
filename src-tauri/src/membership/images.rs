//! Native GPT Image generation through the signed-in Codex membership route.
//! Credentials remain in MembershipService; this module returns bounded PNG bytes only.
use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    agent::CancellationToken,
    tools::{GeneratedImage, ImageGenerationBackend, ToolError},
};

use super::{
    service::{provider_failure, read_body, MembershipService},
    MembershipSelection,
};

const CODEX_IMAGE_ROUTE: &str = "https://chatgpt.com/backend-api/codex/images/generations";
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

pub(crate) struct MembershipImageGenerator {
    service: Arc<MembershipService>,
    selection: MembershipSelection,
}

impl MembershipImageGenerator {
    pub(crate) fn new(service: Arc<MembershipService>, selection: MembershipSelection) -> Self {
        Self { service, selection }
    }
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidRequest {
        tool_name: "generate_image".into(),
        message: message.into(),
    }
}

fn account_headers(token: &str) -> Vec<(String, String)> {
    let mut headers = vec![
        ("User-Agent".into(), "CrowClaw/0.1.0".into()),
        ("originator".into(), "crowclaw".into()),
    ];
    let Some(payload) = token.split('.').nth(1) else {
        return headers;
    };
    let Ok(bytes) = URL_SAFE_NO_PAD.decode(payload) else {
        return headers;
    };
    let Ok(claims) = serde_json::from_slice::<Value>(&bytes) else {
        return headers;
    };
    let auth = &claims["https://api.openai.com/auth"];
    if let Some(account) = auth["chatgpt_account_id"].as_str().filter(|v| {
        !v.is_empty()
            && v.len() <= 256
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    }) {
        headers.push(("ChatGPT-Account-ID".into(), account.into()));
    }
    if let Some(residency) = auth["chatgpt_data_residency"]
        .as_str()
        .or_else(|| auth["chatgpt_compute_residency"].as_str())
        .filter(|v| {
            v.len() <= 64
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        })
    {
        headers.push(("x-openai-internal-codex-residency".into(), residency.into()));
    }
    headers
}

#[async_trait]
impl ImageGenerationBackend for MembershipImageGenerator {
    async fn generate(
        &self,
        prompt: &str,
        quality: &str,
        size: &str,
        cancellation: &CancellationToken,
    ) -> Result<GeneratedImage, ToolError> {
        if self.selection.model != "gpt-6-luna" && self.selection.model != "gpt-6.1-sol" {
            return Err(invalid("Image generation is enabled only for verified GPT-6 Luna and GPT-6.1 Sol membership models"));
        }
        let prompt = prompt.trim();
        if prompt.is_empty()
            || prompt.len() > 16 * 1024
            || !matches!(quality, "low" | "medium" | "high")
            || !matches!(size, "1024x1024" | "1536x1024" | "1024x1536")
        {
            return Err(invalid(
                "Image prompt, quality or size is outside the supported range",
            ));
        }
        let session = self
            .service
            .session_cancellation(&self.selection.account_id)
            .map_err(|_| invalid("The signed-in ChatGPT account is unavailable"))?;
        let credentials = tokio::select! {
            _=cancellation.cancelled()=>return Err(ToolError::Cancelled),
            _=session.cancelled()=>return Err(ToolError::Cancelled),
            result=self.service.codex_image_credentials(&self.selection.account_id,&session)=>result.map_err(|error|invalid(error))?,
        };
        let mut request = self
            .service
            .client
            .post(CODEX_IMAGE_ROUTE)
            .timeout(Duration::from_secs(360))
            .bearer_auth(&credentials.access_token)
            .header("Content-Type", "application/json")
            .header("x-codex-image-turn-id", Uuid::new_v4().to_string());
        for (name, value) in account_headers(&credentials.access_token) {
            request = request.header(name, value);
        }
        let response = tokio::select! {
            _=cancellation.cancelled()=>return Err(ToolError::Cancelled),
            _=session.cancelled()=>return Err(ToolError::Cancelled),
            result=request.json(&json!({"model":"gpt-image-2","prompt":prompt,"n":1,"quality":quality,"size":size,"background":"opaque"})).send()=>result.map_err(|_|invalid("The ChatGPT image service could not be reached"))?,
        };
        let status = response.status();
        if !status.is_success() {
            let bytes = tokio::select! {_ = cancellation.cancelled()=>return Err(ToolError::Cancelled),result=read_body(response,&session,1024*1024)=>result.map_err(|_|invalid("The ChatGPT image service returned an unreadable error"))?};
            return Err(invalid(provider_failure(status.as_u16(), &bytes)));
        }
        let mut response = response;
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::select! {_ = cancellation.cancelled()=>return Err(ToolError::Cancelled),_ = session.cancelled()=>return Err(ToolError::Cancelled),result=response.chunk()=>result.map_err(|_|invalid("The image response was interrupted"))?};
            let Some(chunk) = chunk else { break };
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(invalid(
                    "The image service response exceeded its size limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("The image service returned an invalid response"))?;
        let encoded = body["data"][0]["b64_json"]
            .as_str()
            .ok_or_else(|| invalid("The image service returned no image data"))?;
        if encoded.len() > ((MAX_IMAGE_BYTES + 2) / 3) * 4 + 4 {
            return Err(invalid(
                "Generated image exceeds CrowClaw's 20 MiB storage bound",
            ));
        }
        let image = STANDARD
            .decode(encoded)
            .map_err(|_| invalid("The image service returned invalid image data"))?;
        if image.len() < 24
            || image.len() > MAX_IMAGE_BYTES
            || !image.starts_with(b"\x89PNG\r\n\x1a\n")
        {
            return Err(invalid("Generated image was not a bounded PNG"));
        }
        let width = u32::from_be_bytes(image[16..20].try_into().expect("fixed slice"));
        let height = u32::from_be_bytes(image[20..24].try_into().expect("fixed slice"));
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || width as u64 * height as u64 > 16_000_000
        {
            return Err(invalid(
                "Generated image dimensions exceed CrowClaw's preview bounds",
            ));
        }
        if cancellation.is_cancelled() || session.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(GeneratedImage {
            id: Uuid::new_v4().to_string(),
            name: format!("crowclaw-image-{}.png", Uuid::new_v4().simple()),
            media_type: "image/png".into(),
            model: body["model"]
                .as_str()
                .unwrap_or("gpt-image-2")
                .chars()
                .take(80)
                .collect(),
            bytes: image,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_headers_derive_only_codex_routing_metadata() {
        let payload=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"https://api.openai.com/auth":{"chatgpt_account_id":"acct_123","chatgpt_data_residency":"eu"}})).unwrap());
        let token = format!("header.{payload}.signature");
        assert_eq!(
            account_headers(&token),
            [
                ("User-Agent".into(), "CrowClaw/0.1.0".into()),
                ("originator".into(), "crowclaw".into()),
                ("ChatGPT-Account-ID".into(), "acct_123".into()),
                ("x-openai-internal-codex-residency".into(), "eu".into())
            ]
        );
    }
    #[test]
    fn invalid_or_untrusted_jwt_metadata_never_becomes_a_request_header() {
        assert_eq!(account_headers("not-a-jwt").len(), 2);
        let payload=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"https://api.openai.com/auth":{"chatgpt_account_id":"attacker\r\nInjected: yes"}})).unwrap());
        assert_eq!(account_headers(&format!("h.{payload}.s")).len(), 2);
    }
}
