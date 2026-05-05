mod agent;
mod ollama;
mod prompts;
mod repl;
mod workspace;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use crossterm::style::{style, Color, Stylize};

#[derive(Parser, Debug)]
#[command(name = "monkeycli", version, about = "Copilot-style CLI for Ollama")]
struct Cli {
    #[arg(short, long, global = true)]
    model: Option<String>,

    #[arg(long, global = true)]
    host: Option<String>,

    #[arg(long, global = true)]
    no_stream: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Chat {
        #[arg(required = true)]
        prompt: Vec<String>,
    },
    Code {
        #[arg(required = true)]
        prompt: Vec<String>,
    },
    Repl,
    Agent,
    #[command(name = "code-repl")]
    CodeRepl,
    Models,
    Doctor,
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Assistant,
    Code,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{} {}", style("error:").with(Color::Red), error);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let host = cli
        .host
        .or_else(|| std::env::var("OLLAMA_HOST").ok())
        .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());

    let client = ollama::OllamaClient::new(host);
    let model = resolve_model(&client, cli.model.clone()).await?;

    match cli.command {
        None | Some(Commands::Agent) => agent::start(&client, model).await?,
        Some(Commands::Chat { prompt }) => {
            run_one_shot(&client, &model, Mode::Assistant, prompt.join(" "), cli.no_stream).await?;
        }
        Some(Commands::Code { prompt }) => {
            run_one_shot(&client, &model, Mode::Code, prompt.join(" "), cli.no_stream).await?;
        }
        Some(Commands::Repl) => repl::start(&client, model, Mode::Assistant).await?,
        Some(Commands::CodeRepl) => repl::start(&client, model, Mode::Code).await?,
        Some(Commands::Models) => show_models(&client).await?,
        Some(Commands::Doctor) => doctor(&client, &model).await?,
    }

    Ok(())
}

async fn resolve_model(client: &ollama::OllamaClient, cli_model: Option<String>) -> Result<String> {
    if let Some(model) = cli_model {
        return Ok(model);
    }

    if let Ok(env_model) = std::env::var("OLLAMA_MODEL") {
        if !env_model.trim().is_empty() {
            return Ok(env_model);
        }
    }

    if let Some(model) = client.first_local_model().await? {
        return Ok(model);
    }

    Ok("llama3.1".to_string())
}

async fn run_one_shot(
    client: &ollama::OllamaClient,
    model: &str,
    mode: Mode,
    prompt: String,
    no_stream: bool,
) -> Result<()> {
    if prompt.trim().is_empty() {
        anyhow::bail!("prompt missing");
    }

    let messages = vec![
        ollama::Message::system(prompts::system_prompt(mode)),
        ollama::Message::user(prompt),
    ];

    let answer = client.chat(model, messages, !no_stream).await?;
    if no_stream {
        println!("{answer}");
    } else {
        println!();
    }

    Ok(())
}

async fn show_models(client: &ollama::OllamaClient) -> Result<()> {
    let models = client.list_models().await?;
    if models.is_empty() {
        println!("Aucun modele local trouve.");
        return Ok(());
    }

    println!("Modeles locaux:");
    for model in models {
        println!("- {model}");
    }

    Ok(())
}

async fn doctor(client: &ollama::OllamaClient, model: &str) -> Result<()> {
    println!("{} {}", style("host:").with(Color::Cyan), client.host());

    let models = client.list_models().await.context("cannot list local Ollama models")?;
    if models.is_empty() {
        println!("{} aucun modele local detecte", style("models:").with(Color::Yellow));
        println!("{} smoke test ignore car aucun modele n'est disponible", style("smoke:").with(Color::DarkGrey));
        return Ok(());
    }

    println!("{} {} modele(s) local(aux)", style("models:").with(Color::Green), models.len());
    println!("{} {}", style("default:").with(Color::Cyan), model);

    let smoke = client
        .chat(
            model,
            vec![
                ollama::Message::system("Tu reponds uniquement OK."),
                ollama::Message::user("ping"),
            ],
            false,
        )
        .await;

    match smoke {
        Ok(response) => println!("{} {}", style("smoke:").with(Color::Green), response.trim()),
        Err(error) => println!("{} {}", style("smoke:").with(Color::Red), error),
    }

    Ok(())
}
