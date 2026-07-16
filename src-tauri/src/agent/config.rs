use serde::{Deserialize, Serialize};

use super::ProviderError;

const DEFAULT_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPreset {
    LmStudio,
    Ollama,
    LlamaCpp,
    Custom,
}

impl ProviderPreset {
    pub fn default_base_url(&self) -> Option<&'static str> {
        match self {
            Self::LmStudio => Some("http://127.0.0.1:1234/v1"),
            Self::Ollama => Some("http://127.0.0.1:11434/v1"),
            Self::LlamaCpp => Some("http://127.0.0.1:8080/v1"),
            Self::Custom => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub preset: ProviderPreset,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub request_timeout_ms: u64,
    pub max_response_bytes: usize,
}

impl ProviderConfig {
    pub fn for_preset(preset: ProviderPreset) -> Result<Self, ProviderError> {
        let base_url =
            preset
                .default_base_url()
                .ok_or_else(|| ProviderError::InvalidConfiguration {
                    message: "a custom provider requires an explicit base URL".into(),
                })?;

        Ok(Self {
            preset,
            base_url: base_url.into(),
            api_key: None,
            default_model: None,
            request_timeout_ms: DEFAULT_TIMEOUT_MS,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        })
    }

    pub fn custom(base_url: impl Into<String>) -> Self {
        Self {
            preset: ProviderPreset::Custom,
            base_url: base_url.into(),
            api_key: None,
            default_model: None,
            request_timeout_ms: DEFAULT_TIMEOUT_MS,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }

    pub fn normalized_base_url(&self) -> Result<String, ProviderError> {
        let trimmed = self.base_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(ProviderError::InvalidConfiguration {
                message: "base URL cannot be empty".into(),
            });
        }

        let parsed =
            reqwest::Url::parse(trimmed).map_err(|error| ProviderError::InvalidConfiguration {
                message: format!("base URL is not valid: {error}"),
            })?;

        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ProviderError::InvalidConfiguration {
                message: "base URL must use http or https".into(),
            });
        }
        if parsed.host_str().is_none() {
            return Err(ProviderError::InvalidConfiguration {
                message: "base URL must include a host".into(),
            });
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(ProviderError::InvalidConfiguration {
                message: "put credentials in the API-key field, not the URL".into(),
            });
        }
        if self.request_timeout_ms == 0 {
            return Err(ProviderError::InvalidConfiguration {
                message: "request timeout must be greater than zero".into(),
            });
        }
        if self.max_response_bytes == 0 {
            return Err(ProviderError::InvalidConfiguration {
                message: "response-size boundary must be greater than zero".into(),
            });
        }

        Ok(trimmed.to_string())
    }

    pub fn endpoint(&self, path: &str) -> Result<String, ProviderError> {
        Ok(format!(
            "{}/{}",
            self.normalized_base_url()?,
            path.trim_start_matches('/')
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderConfig, ProviderPreset};

    #[test]
    fn local_presets_use_openai_compatible_v1_endpoints() {
        let cases = [
            (ProviderPreset::LmStudio, "http://127.0.0.1:1234/v1"),
            (ProviderPreset::Ollama, "http://127.0.0.1:11434/v1"),
            (ProviderPreset::LlamaCpp, "http://127.0.0.1:8080/v1"),
        ];

        for (preset, expected) in cases {
            let config = ProviderConfig::for_preset(preset).expect("preset should be valid");
            assert_eq!(config.base_url, expected);
            assert_eq!(
                config.endpoint("models").unwrap(),
                format!("{expected}/models")
            );
        }
    }

    #[test]
    fn rejects_credentials_embedded_in_url() {
        let config = ProviderConfig::custom("http://token@example.test/v1");
        assert!(config.normalized_base_url().is_err());
    }
}
