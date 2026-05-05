use anyhow::{anyhow, Context, Result};
use crossterm::style::{style, Color, Stylize};
use serde::Deserialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::{ollama, prompts, workspace};

#[derive(Debug, Deserialize)]
struct EditPlan {
    summary: String,
    #[serde(default)]
    changes: Vec<FileChange>,
}

#[derive(Debug, Deserialize)]
struct FileChange {
    path: String,
    action: ChangeAction,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ChangeAction {
    Create,
    Update,
    Delete,
}

pub async fn start(client: &ollama::OllamaClient, model: String) -> Result<()> {
    let workspace_root = std::env::current_dir().context("failed to read current directory")?;
    let mut model = model;

    println!(
        "{} {}",
        style("MonkeyCLI").with(Color::Cyan).bold(),
        style("(agent)").with(Color::DarkGrey)
    );
    println!("Tape une demande pour modifier les fichiers du workspace.");
    println!("Commandes: /help, /exit, /model <nom>");

    loop {
        print!("{} ", style(">").with(Color::Green).bold());
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }

        let line = input.trim();
        if line.is_empty() {
            continue;
        }

        match line {
            "/exit" | "/quit" => break,
            "/help" => {
                println!("/help            Affiche l'aide");
                println!("/model <nom>     Change le modele actif");
                println!("/exit            Quitte");
                continue;
            }
            _ if line.starts_with("/model ") => {
                let next = line.trim_start_matches("/model ").trim();
                if !next.is_empty() {
                    model = next.to_string();
                    println!("Modele actif: {model}");
                }
                continue;
            }
            _ => {}
        }

        run_agent_turn(client, &model, &workspace_root, line).await?;
    }

    Ok(())
}

async fn run_agent_turn(client: &ollama::OllamaClient, model: &str, workspace_root: &Path, request: &str) -> Result<()> {
    let context = workspace::collect_workspace_context(workspace_root)?;
    let prompt = build_agent_prompt(request, &context);

    let messages = vec![
        ollama::Message::system(prompts::agent_system_prompt()),
        ollama::Message::user(prompt),
    ];

    let response = client.chat(model, messages, false).await?;
    let plan = parse_plan(&response)?;

    if plan.changes.is_empty() {
        println!("{} Aucune modification proposee.", style("plan:").with(Color::Yellow));
        println!("{} {}", style("summary:").with(Color::Cyan), plan.summary);
        return Ok(());
    }

    print_plan(&plan, workspace_root)?;

    if !ask_confirmation("Appliquer ces modifications ? [y/N] ")? {
        println!("Modifications abandonnees.");
        return Ok(());
    }

    apply_plan(&plan, workspace_root)?;
    println!("Modifications appliquees.");
    Ok(())
}

fn build_agent_prompt(request: &str, context: &str) -> String {
    format!(
        "TASK:\n{}\n\nWORKSPACE CONTEXT:\n{}\n\nReturn strict JSON only.",
        request, context
    )
}

fn parse_plan(response: &str) -> Result<EditPlan> {
    let json_text = extract_json_object(response).ok_or_else(|| anyhow!("the model did not return valid JSON"))?;
    let plan: EditPlan = serde_json::from_str(json_text).context("failed to parse edit plan JSON")?;
    Ok(plan)
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
}

fn print_plan(plan: &EditPlan, workspace_root: &Path) -> Result<()> {
    println!("\n{} {}", style("summary:").with(Color::Cyan), plan.summary);

    for change in &plan.changes {
        let path = normalize_path(workspace_root, &change.path);
        let action = match change.action {
            ChangeAction::Create => "create",
            ChangeAction::Update => "update",
            ChangeAction::Delete => "delete",
        };

        println!("\n{} {} [{}]", style("file:").with(Color::Green), path.display(), action);
        if let Some(note) = &change.note {
            println!("{} {}", style("note:").with(Color::DarkGrey), note);
        }

        match change.action {
            ChangeAction::Delete => {
                if let Ok(existing) = fs::read_to_string(&path) {
                    let diff = similar::TextDiff::from_lines(existing.as_str(), "")
                        .unified_diff()
                        .header(&format!("a/{}", change.path), &format!("b/{}", change.path))
                        .context_radius(3)
                        .to_string();
                    println!("{diff}");
                }
            }
            ChangeAction::Create | ChangeAction::Update => {
                if let Some(content) = &change.content {
                    let existing = fs::read_to_string(&path).unwrap_or_default();
                    let diff = similar::TextDiff::from_lines(&existing, content)
                        .unified_diff()
                        .header(&format!("a/{}", change.path), &format!("b/{}", change.path))
                        .context_radius(3)
                        .to_string();
                    println!("{diff}");
                } else {
                    println!("(no content provided)");
                }
            }
        }
    }

    Ok(())
}

fn ask_confirmation(message: &str) -> Result<bool> {
    print!("{message}");
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes" | "o" | "oui"))
}

fn apply_plan(plan: &EditPlan, workspace_root: &Path) -> Result<()> {
    for change in &plan.changes {
        let path = normalize_path(workspace_root, &change.path);
        match change.action {
            ChangeAction::Create | ChangeAction::Update => {
                let content = change.content.as_ref().ok_or_else(|| anyhow!("missing content for {}", change.path))?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))?;
            }
            ChangeAction::Delete => {
                if path.exists() {
                    fs::remove_file(&path).with_context(|| format!("failed to delete {}", path.display()))?;
                }
            }
        }
    }

    Ok(())
}

fn normalize_path(root: &Path, relative: &str) -> PathBuf {
    let cleaned = relative.replace('/', std::path::MAIN_SEPARATOR_STR);
    root.join(cleaned)
}
