use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use roughneck_core::{
    ChatMessage, DeepAgentConfig, FileSystemBackendKind, ModelProviderConfig, SessionInit,
    SessionInvokeRequest,
};
use roughneck_runtime::{AgentSession, DeepAgent};
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, ValueEnum)]
enum Provider {
    Openai,
    Anthropic,
}

#[derive(Debug, Parser)]
#[command(name = "roughneck")]
#[command(about = "Rig-backed deep-agent harness")]
struct Cli {
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    local_fs: bool,
    #[arg(long)]
    allow_execute: bool,
    #[arg(long, value_enum, default_value_t = Provider::Openai)]
    provider: Provider,
    #[arg(long)]
    model: Option<String>,
    #[arg(long, default_value_t = 24)]
    max_turns: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let mut config = if let Some(path) = args.config.as_ref() {
        load_config(path)?
    } else {
        DeepAgentConfig::default()
    };

    config.max_turns = args.max_turns;
    config.filesystem.execute.enabled = args.allow_execute;

    if args.local_fs {
        config.filesystem.backend = FileSystemBackendKind::Local {
            root: std::env::current_dir().context("failed to resolve current directory")?,
        };
        config.filesystem.snapshot_on_response = Some(false);
    }

    let model = args.model.unwrap_or_else(|| match args.provider {
        Provider::Openai => "gpt-4o-mini".to_string(),
        Provider::Anthropic => "claude-3-5-sonnet-latest".to_string(),
    });

    config.model = match args.provider {
        Provider::Openai => ModelProviderConfig::OpenAi {
            model,
            api_key_env: "OPENAI_API_KEY".to_string(),
        },
        Provider::Anthropic => ModelProviderConfig::Anthropic {
            model,
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
        },
    };

    let agent = DeepAgent::new(config).context("failed to initialize deep agent")?;
    let session = agent
        .start_session(SessionInit {
            session_id: Some("cli".to_string()),
            ..SessionInit::default()
        })
        .await
        .context("failed to start session")?;

    if let Some(prompt) = args.prompt {
        run_single(&session, prompt).await
    } else {
        run_interactive(&session).await
    }
}

async fn run_single(session: &AgentSession, prompt: String) -> Result<()> {
    let response = session
        .invoke(SessionInvokeRequest {
            messages: vec![ChatMessage::user(prompt)],
        })
        .await
        .context("invoke failed")?;

    if let Some(last) = response.latest_assistant_message {
        println!("{}", last.content);
    }
    Ok(())
}

async fn run_interactive(session: &AgentSession) -> Result<()> {
    loop {
        print!("> ");
        io::stdout().flush().context("failed to flush stdout")?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("failed to read line")?;

        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
            break;
        }
        if trimmed.is_empty() {
            continue;
        }

        let response = session
            .invoke(SessionInvokeRequest {
                messages: vec![ChatMessage::user(trimmed)],
            })
            .await
            .context("invoke failed")?;

        if let Some(last) = response.latest_assistant_message {
            println!("{}", last.content);
        }
    }

    Ok(())
}

fn load_config(path: &PathBuf) -> Result<DeepAgentConfig> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse config {}", path.display()))
}
