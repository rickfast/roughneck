use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    #[default]
    Disabled,
    Experimental,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelProviderConfig {
    OpenAi {
        model: String,
        #[serde(default = "default_openai_api_key_env")]
        api_key_env: String,
    },
    Anthropic {
        model: String,
        #[serde(default = "default_anthropic_api_key_env")]
        api_key_env: String,
    },
}

fn default_openai_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_anthropic_api_key_env() -> String {
    "ANTHROPIC_API_KEY".to_string()
}

impl Default for ModelProviderConfig {
    fn default() -> Self {
        Self::OpenAi {
            model: "gpt-4o-mini".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileSystemBackendKind {
    InMemory,
    Local { root: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteConfig {
    pub enabled: bool,
    pub default_timeout_secs: u64,
    pub max_timeout_secs: u64,
}

impl Default for ExecuteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_timeout_secs: 10,
            max_timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSystemConfig {
    pub backend: FileSystemBackendKind,
    #[serde(default)]
    pub execute: ExecuteConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_on_response: Option<bool>,
}

impl FileSystemConfig {
    /// Returns whether the runtime should include a filesystem snapshot in responses.
    #[must_use]
    pub fn snapshot_on_response(&self) -> bool {
        self.snapshot_on_response.unwrap_or(match self.backend {
            FileSystemBackendKind::InMemory => true,
            FileSystemBackendKind::Local { .. } => false,
        })
    }
}

impl Default for FileSystemConfig {
    fn default() -> Self {
        Self {
            backend: FileSystemBackendKind::InMemory,
            execute: ExecuteConfig::default(),
            snapshot_on_response: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryBackendKind {
    InMemory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub backend: MemoryBackendKind,
    pub short_term_limit: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: MemoryBackendKind::InMemory,
            short_term_limit: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default)]
    pub enabled_skills: Vec<String>,
    #[serde(default)]
    pub registry_paths: Vec<PathBuf>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled_skills: Vec::new(),
            registry_paths: vec![PathBuf::from("skills")],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookRule {
    #[serde(default = "default_hook_matcher")]
    pub matcher: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

fn default_hook_matcher() -> String {
    "*".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HooksConfig {
    pub enabled: bool,
    pub timeout_secs: u64,
    #[serde(default)]
    pub pre_tool_use: Vec<HookRule>,
    #[serde(default)]
    pub post_tool_use: Vec<HookRule>,
    #[serde(default)]
    pub notification: Vec<HookRule>,
    #[serde(default)]
    pub stop: Vec<HookRule>,
    #[serde(default)]
    pub subagent_stop: Vec<HookRule>,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: 30,
            pre_tool_use: Vec::new(),
            post_tool_use: Vec::new(),
            notification: Vec::new(),
            stop: Vec::new(),
            subagent_stop: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentConfig {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentsConfig {
    #[serde(default)]
    pub status: CapabilityStatus,
    #[serde(default = "default_max_subagent_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub agents: Vec<SubagentConfig>,
}

fn default_max_subagent_depth() -> usize {
    2
}

impl Default for SubagentsConfig {
    fn default() -> Self {
        Self {
            status: CapabilityStatus::Disabled,
            max_depth: default_max_subagent_depth(),
            agents: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub endpoint: Url,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub status: CapabilityStatus,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub enable_meta_tool: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            status: CapabilityStatus::Disabled,
            servers: Vec::new(),
            enable_meta_tool: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeepAgentConfig {
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default)]
    pub model: ModelProviderConfig,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub filesystem: FileSystemConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub subagents: SubagentsConfig,
    #[serde(default)]
    pub mcp: McpConfig,
    #[serde(default)]
    pub hooks: HooksConfig,
}

fn default_max_turns() -> usize {
    24
}

fn default_system_prompt() -> String {
    "You are Roughneck, a deep agent built on Rig.".to_string()
}

impl Default for DeepAgentConfig {
    fn default() -> Self {
        Self {
            system_prompt: default_system_prompt(),
            model: ModelProviderConfig::default(),
            max_turns: default_max_turns(),
            max_tokens: Some(2048),
            filesystem: FileSystemConfig::default(),
            memory: MemoryConfig::default(),
            skills: SkillsConfig::default(),
            subagents: SubagentsConfig::default(),
            mcp: McpConfig::default(),
            hooks: HooksConfig::default(),
        }
    }
}
