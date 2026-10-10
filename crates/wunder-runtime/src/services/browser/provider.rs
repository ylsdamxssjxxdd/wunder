//! dsh-style browser provider abstraction.
//!
//! Exactly one provider may be registered into the process-wide
//! [`BrowserUseRegistry`]. The provider owns the upstream-native tool surface
//! (tool names + JSON schemas) and turns a single call into MCP-style content
//! blocks.
//!
//! wunder keeps its own safety defaults (private-network / `file://` blocking,
//! viewport, timeout and download limits) inside the runtime layer, so providers
//! do not have to re-implement them.

use crate::config::Config;
use crate::schemas::ToolSpec;
use crate::services::browser::model::BrowserSessionScope;
use crate::services::tools::ToolContext;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use tracing::{error, warn};

/// One MCP-style content block produced by a provider call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text block.
    Text { text: String },
    /// Inline image block; `data` is base64 encoded.
    Image { data: String, mime_type: String },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        ContentBlock::Image {
            data: data.into(),
            mime_type: mime_type.into(),
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(self, ContentBlock::Image { .. })
    }
}

/// Outcome of a single provider tool call.
#[derive(Debug, Clone)]
pub struct ProviderOutcome {
    pub ok: bool,
    pub blocks: Vec<ContentBlock>,
    pub error: Option<String>,
    /// Raw provider payload kept for downstream sanitising / inspection.
    pub structured: Option<Value>,
}

impl ProviderOutcome {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            blocks: vec![ContentBlock::text(text)],
            error: None,
            structured: None,
        }
    }

    pub fn blocks(blocks: Vec<ContentBlock>) -> Self {
        Self {
            ok: true,
            blocks,
            error: None,
            structured: None,
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            ok: false,
            blocks: vec![ContentBlock::text(message.clone())],
            error: Some(message),
            structured: None,
        }
    }

    pub fn with_structured(mut self, value: Value) -> Self {
        self.structured = Some(value);
        self
    }

    /// Serialise into the model-facing JSON envelope.
    pub fn to_value(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("ok".to_string(), Value::Bool(self.ok));
        map.insert(
            "content".to_string(),
            serde_json::to_value(&self.blocks).unwrap_or_else(|_| Value::Array(Vec::new())),
        );
        if let Some(error) = &self.error {
            map.insert("error".to_string(), Value::String(error.clone()));
        }
        if let Some(structured) = &self.structured {
            map.insert("data".to_string(), structured.clone());
        }
        Value::Object(map)
    }
}

/// Boxed future returned by [`BrowserProvider::call`].
pub type ProviderCallFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ProviderOutcome>> + Send + 'a>>;

/// A browser backend that owns a native tool surface.
pub trait BrowserProvider: Send + Sync {
    /// Stable machine id, e.g. `playwright`.
    fn id(&self) -> &'static str;

    /// Human readable name for status output.
    fn display_name(&self) -> &'static str;

    /// Native tool specs exposed to the model.
    fn tool_specs(&self) -> Vec<ToolSpec>;

    /// Whether this provider owns `name`.
    fn owns_tool(&self, name: &str) -> bool {
        self.tool_specs()
            .iter()
            .any(|spec| spec.name.as_str() == name.trim())
    }

    /// Execute `tool` with `args`.
    fn call<'a, 'ctx>(
        &'a self,
        context: &'a ToolContext<'ctx>,
        tool: &'a str,
        args: &'a Value,
    ) -> ProviderCallFuture<'a>;
}

/// Exclusive registration slot for the active browser provider.
pub struct BrowserUseRegistry {
    provider: OnceLock<Arc<dyn BrowserProvider>>,
}

impl BrowserUseRegistry {
    pub fn new() -> Self {
        Self {
            provider: OnceLock::new(),
        }
    }

    /// Register the one and only provider. A second registration is rejected.
    pub fn register(&self, provider: Arc<dyn BrowserProvider>) -> Result<()> {
        let id = provider.id();
        self.provider.set(provider).map_err(|_| {
            anyhow!("browser provider already registered; rejected duplicate '{id}'")
        })
    }

    pub fn provider(&self) -> Option<Arc<dyn BrowserProvider>> {
        self.provider.get().cloned()
    }

    pub fn is_registered(&self) -> bool {
        self.provider.get().is_some()
    }
}

impl Default for BrowserUseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static BROWSER_REGISTRY: OnceLock<BrowserUseRegistry> = OnceLock::new();

/// Process-wide registry holding the builtin provider.
pub fn browser_registry() -> &'static BrowserUseRegistry {
    BROWSER_REGISTRY.get_or_init(|| {
        let registry = BrowserUseRegistry::new();
        let provider = Arc::new(
            crate::services::browser::providers::PlaywrightProvider::new(),
        );
        if let Err(err) = registry.register(provider) {
            error!(error = %err, "failed to register builtin browser provider");
        }
        registry
    })
}

/// Resolve the provider selected by `browser.provider`, falling back to the
/// registered provider when the configured id is unknown.
pub fn active_provider(config: &Config) -> Option<Arc<dyn BrowserProvider>> {
    let provider = browser_registry().provider()?;
    let wanted = config.browser.provider.trim();
    if !wanted.is_empty() && !wanted.eq_ignore_ascii_case(provider.id()) {
        warn!(
            configured = wanted,
            active = provider.id(),
            "unknown browser provider requested; falling back to the registered provider"
        );
    }
    Some(provider)
}

/// Native tool names of the active provider (empty when none registered).
pub fn registered_tool_names() -> Vec<String> {
    browser_registry()
        .provider()
        .map(|provider| {
            provider
                .tool_specs()
                .into_iter()
                .map(|spec| spec.name)
                .collect()
        })
        .unwrap_or_default()
}

/// Derive the browser session scope for a tool call.
pub fn scope_from_context(context: &ToolContext<'_>, args: &Value) -> BrowserSessionScope {
    BrowserSessionScope {
        user_id: context.user_id.to_string(),
        session_id: context.session_id.to_string(),
        agent_id: context.agent_id.map(ToString::to_string),
        profile: args
            .get("profile")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        browser_session_id: args
            .get("browser_session_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    }
}