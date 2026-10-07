use std::{
    error::Error,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::chunker;
use crate::agent::CancellationToken;

pub const EMBEDDING_CODEC: &str = "f32le-normalized-v1";
pub const EMBEDDING_BATCH: usize = 8;
pub const EMBEDDING_TIMEOUT: Duration = Duration::from_secs(15);
const RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddingProvider {
    #[serde(rename = "openai")]
    OpenAi,
    #[serde(rename = "ollama")]
    Ollama,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingProfile {
    pub provider: EmbeddingProvider,
    pub base_url: String,
    pub model: String,
    pub dimensions: u32,
}

impl EmbeddingProfile {
    pub fn validate(&self) -> Result<(), String> {
        self.normalized().map(|_| ())
    }

    pub fn normalized(&self) -> Result<Self, String> {
        if self.base_url.len() > 1024 {
            return Err("Embedding endpoint is too long".into());
        }
        let url = Url::parse(self.base_url.trim())
            .map_err(|_| "Embedding endpoint is not a valid URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Embedding endpoint must be HTTP(S) without credentials, query parameters or fragments".into());
        }
        let host = url
            .host_str()
            .ok_or("Embedding endpoint needs a loopback host")?
            .trim_matches(['[', ']']);
        if host != "localhost" && !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
            return Err("Semantic memory accepts only loopback endpoints on this computer".into());
        }
        if !(1..=4096).contains(&self.dimensions) {
            return Err("Embedding dimensions must be 1–4096".into());
        }
        let model = self.model.trim();
        if model.is_empty() || model.len() > 256 {
            return Err("Enter an embedding model identifier of 1–256 bytes".into());
        }
        Ok(Self {
            provider: self.provider,
            base_url: url.as_str().trim_end_matches('/').into(),
            model: model.into(),
            dimensions: self.dimensions,
        })
    }

    pub fn id(&self) -> Result<String, String> {
        let normalized = self.normalized()?;
        Ok(chunker::hash(&format!(
            "crowclaw.embedding.profile.v1\0{EMBEDDING_CODEC}\0{}",
            serde_json::to_string(&normalized).map_err(|_| "Cannot encode embedding profile")?
        )))
    }
}

pub(super) struct EmbeddingClient {
    profile: EmbeddingProfile,
    client: Client,
}

impl EmbeddingClient {
    pub fn new(profile: &EmbeddingProfile) -> Result<Self, String> {
        let profile = profile.normalized()?;
        let mut builder = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(EMBEDDING_TIMEOUT);
        let url = Url::parse(&profile.base_url).map_err(|_| "Invalid embedding URL")?;
        if url.host_str() == Some("localhost") {
            // Preserve the hostname for TLS certificates while pinning its
            // connection address; the client never asks DNS for localhost.
            builder = builder.resolve(
                "localhost",
                SocketAddr::new(
                    Ipv4Addr::LOCALHOST.into(),
                    url.port_or_known_default()
                        .ok_or("Embedding endpoint has no port")?,
                ),
            );
        }
        let client = builder
            .build()
            .map_err(|_| "Could not create local embedding client")?;
        Ok(Self { profile, client })
    }

    pub async fn embed(
        &self,
        inputs: &[String],
        token: &CancellationToken,
        feature_token: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, String> {
        if inputs.is_empty()
            || inputs.len() > EMBEDDING_BATCH
            || inputs.iter().any(|s| s.trim().is_empty() || s.len() > 4096)
        {
            return Err(
                "Embedding batch must contain 1–8 nonempty texts of at most 4096 bytes".into(),
            );
        }
        if token.is_cancelled() || feature_token.is_cancelled() {
            return Err("Embedding request cancelled".into());
        }
        let endpoint = match self.profile.provider {
            EmbeddingProvider::OpenAi => format!("{}/embeddings", self.profile.base_url),
            EmbeddingProvider::Ollama => {
                let mut url =
                    Url::parse(&self.profile.base_url).map_err(|_| "Invalid embedding URL")?;
                url.set_path("/api/embed");
                url.to_string()
            }
        };
        let body = match self.profile.provider {
            // Do not ask for dimensional truncation; validate the actual model
            // output against the explicitly chosen profile instead.
            EmbeddingProvider::OpenAi => {
                json!({"model":self.profile.model,"input":inputs,"encoding_format":"float"})
            }
            EmbeddingProvider::Ollama => {
                json!({"model":self.profile.model,"input":inputs,"truncate":false})
            }
        };
        let request = async {
            let mut response = self
                .client
                .post(endpoint)
                .json(&body)
                .send()
                .await
                .map_err(|error| {
                    let mut source = error.source();
                    let mut reason = "connection failed".to_string();
                    while let Some(cause) = source {
                        reason = cause.to_string();
                        source = cause.source();
                    }
                    format!(
                        "Local embedding endpoint is unavailable ({})",
                        reason.chars().take(256).collect::<String>()
                    )
                })?;
            if !response.status().is_success() {
                return Err(format!(
                    "Local embedding endpoint returned HTTP {}",
                    response.status().as_u16()
                ));
            }
            if response
                .content_length()
                .is_some_and(|n| n > RESPONSE_MAX_BYTES as u64)
            {
                return Err("Embedding response exceeds 4 MiB".into());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "Cannot read embedding response")?
            {
                if bytes.len().saturating_add(chunk.len()) > RESPONSE_MAX_BYTES {
                    return Err("Embedding response exceeds 4 MiB".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let value: Value =
                serde_json::from_slice(&bytes).map_err(|_| "Embedding response is invalid JSON")?;
            decode_response(&self.profile, &value, inputs.len())
        };
        tokio::select! {
            _=token.cancelled()=>Err("Embedding request cancelled".into()),
            _=feature_token.cancelled()=>Err("Embedding profile changed or was disabled".into()),
            result=tokio::time::timeout(EMBEDDING_TIMEOUT,request)=>result.map_err(|_|"Embedding request timed out after 15 seconds")?,
        }
    }
}

fn decode_response(
    profile: &EmbeddingProfile,
    value: &Value,
    count: usize,
) -> Result<Vec<Vec<f32>>, String> {
    if value.get("model").and_then(Value::as_str) != Some(profile.model.as_str()) {
        return Err("Embedding response model does not match the selected profile".into());
    }
    let values = match profile.provider {
        EmbeddingProvider::Ollama => value
            .get("embeddings")
            .and_then(Value::as_array)
            .ok_or("Embedding response has no vector batch")?
            .clone(),
        EmbeddingProvider::OpenAi => {
            let rows = value
                .get("data")
                .and_then(Value::as_array)
                .ok_or("Embedding response has no data batch")?;
            if rows.len() != count {
                return Err("Embedding response count does not match the request".into());
            }
            let mut sorted = vec![None; count];
            for row in rows {
                let index = row
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or("Embedding response has no input index")?
                    as usize;
                if index >= count || sorted[index].is_some() {
                    return Err("Embedding response indexes are duplicated or out of range".into());
                }
                sorted[index] = Some(
                    row.get("embedding")
                        .ok_or("Embedding row has no vector")?
                        .clone(),
                );
            }
            sorted
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .ok_or("Embedding response has missing indexes")?
        }
    };
    if values.len() != count {
        return Err("Embedding response count does not match the request".into());
    }
    values
        .iter()
        .map(|value| {
            let array = value
                .as_array()
                .ok_or("Embedding vector is not a number array")?;
            if array.len() != profile.dimensions as usize {
                return Err(
                    "Embedding response dimensions do not match the selected profile".into(),
                );
            }
            let numbers = array
                .iter()
                .map(|v| {
                    v.as_f64()
                        .filter(|n| n.is_finite())
                        .ok_or("Embedding contains a nonfinite or nonnumeric value".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            normalize(&numbers)
        })
        .collect()
}

fn normalize(values: &[f64]) -> Result<Vec<f32>, String> {
    let scale = values.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if scale == 0.0 || !scale.is_finite() {
        return Err("Embedding vector has zero or invalid norm".into());
    }
    let norm = values
        .iter()
        .map(|v| (v / scale).powi(2))
        .sum::<f64>()
        .sqrt();
    let result = values
        .iter()
        .map(|v| (v / scale / norm) as f32)
        .collect::<Vec<_>>();
    if result.iter().any(|v| !v.is_finite()) {
        return Err("Normalized embedding contains invalid values".into());
    }
    Ok(result)
}

pub(super) fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub(super) fn decode(bytes: &[u8], dimensions: u32) -> Result<Vec<f32>, String> {
    if !(1..=4096).contains(&dimensions) || bytes.len() != dimensions as usize * 4 {
        return Err("Stored semantic vector size is invalid".into());
    }
    let values = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().expect("exact four-byte chunk")))
        .collect::<Vec<_>>();
    if values.iter().any(|v| !v.is_finite()) {
        return Err("Stored semantic vector contains nonfinite values".into());
    }
    let norm = values.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    if (norm - 1.0).abs() > 0.001 {
        return Err("Stored semantic vector is not normalized".into());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_numeric_responses_and_normalizes_without_overflow() {
        let p = EmbeddingProfile {
            provider: EmbeddingProvider::OpenAi,
            base_url: "http://127.0.0.1/v1".into(),
            model: "fixture".into(),
            dimensions: 2,
        };
        let vectors = decode_response(
            &p,
            &json!({"model":"fixture","data":[{"index":0,"embedding":[1e300,1e300]}]}),
            1,
        )
        .unwrap();
        assert!((vectors[0][0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
        for bad in [json!([0.0, 0.0]), json!(["NaN", 1.0]), json!([1.0])] {
            assert!(decode_response(
                &p,
                &json!({"model":"fixture","data":[{"index":0,"embedding":bad}]}),
                1
            )
            .is_err());
        }
        assert!(decode(&[0; 8], 2).is_err());
        assert!(decode(&f32::NAN.to_le_bytes(), 1).is_err());
        assert_eq!(decode(&encode(&[1.0, 0.0]), 2).unwrap(), vec![1.0, 0.0]);
    }
}
