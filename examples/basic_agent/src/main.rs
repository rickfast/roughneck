use anyhow::{Context, Result, bail};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use roughneck_core::{
    ChatMessage, DeepAgentConfig, ModelProviderConfig, RoughneckError, SessionInit,
    SessionInvokeRequest,
};
use roughneck_runtime::DeepAgent;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

const DEFAULT_PROMPT: &str = "Inspect the seeded files, summarize the project state, and propose the next two engineering tasks.";

#[tokio::main]
async fn main() -> Result<()> {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_PROMPT.to_string());

    let agent = DeepAgent::new(example_config()?)
        .map(|agent| agent.with_tool(ReleaseLookupTool))
        .context("failed to initialize roughneck example agent")?;
    let session = agent
        .start_session(SessionInit {
            session_id: Some("basic-example".to_string()),
            initial_files: seeded_files(),
            ..SessionInit::default()
        })
        .await
        .context("failed to start example session")?;

    let response = session
        .invoke(SessionInvokeRequest {
            messages: vec![ChatMessage::user(prompt)],
        })
        .await
        .context("example agent invocation failed")?;

    println!("session: {}", response.session_id);
    if let Some(message) = response.latest_assistant_message {
        println!("\nassistant:\n{}", message.content);
    }

    if let Some(snapshot) = response.workspace_snapshot {
        println!("\nworkspace snapshot:");
        let mut paths = snapshot.keys().cloned().collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            println!("- {path}");
        }
    }

    Ok(())
}

fn example_config() -> Result<DeepAgentConfig> {
    let mut config = DeepAgentConfig {
        system_prompt: "You are demonstrating Roughneck's session-oriented deep-agent runtime."
            .to_string(),
        ..DeepAgentConfig::default()
    };
    config.model = model_config_from_env()?;
    Ok(config)
}

fn model_config_from_env() -> Result<ModelProviderConfig> {
    let provider = std::env::var("ROUGHNECK_PROVIDER")
        .unwrap_or_else(|_| "openai".to_string())
        .to_ascii_lowercase();

    match provider.as_str() {
        "openai" => Ok(ModelProviderConfig::OpenAi {
            model: std::env::var("ROUGHNECK_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            api_key_env: "OPENAI_API_KEY".to_string(),
        }),
        "anthropic" => Ok(ModelProviderConfig::Anthropic {
            model: std::env::var("ROUGHNECK_MODEL")
                .unwrap_or_else(|_| "claude-3-5-sonnet-latest".to_string()),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
        }),
        other => bail!(
            "unsupported ROUGHNECK_PROVIDER value '{other}', expected 'openai' or 'anthropic'"
        ),
    }
}

fn seeded_files() -> HashMap<String, String> {
    HashMap::from([
        (
            "README.md".to_string(),
            "# Roughneck Example\n\nThis in-memory workspace exists only for the example crate.\n"
                .to_string(),
        ),
        (
            "src/lib.rs".to_string(),
            "pub fn greet(name: &str) -> String {\n    format!(\"hello, {name}!\")\n}\n"
                .to_string(),
        ),
        (
            "notes/todos.md".to_string(),
            "- Add stronger tests\n- Wire a real MCP transport\n".to_string(),
        ),
    ])
}

#[derive(Debug, Serialize, Deserialize)]
struct ReleaseLookupArgs {
    name: String,
}

#[derive(Debug, Clone, Copy)]
struct ReleaseLookupTool;

impl Tool for ReleaseLookupTool {
    const NAME: &'static str = "lookup_release";
    type Error = RoughneckError;
    type Args = ReleaseLookupArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Return a canned release version for a package name.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> std::result::Result<Self::Output, Self::Error> {
        let version = match args.name.as_str() {
            "roughneck" => "0.1.0",
            _ => "unknown",
        };
        Ok(json!({
            "name": args.name,
            "version": version,
        }))
    }
}
