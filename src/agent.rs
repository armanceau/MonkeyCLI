use anyhow::{anyhow, Context, Result};
use crossterm::style::{style, Color, Stylize};
use serde::Deserialize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

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
    let start_time = Instant::now();
    
    let context = workspace::collect_workspace_context(workspace_root)?;
    let prompt = build_agent_prompt(request, &context);

    let messages = vec![
        ollama::Message::system(prompts::agent_system_prompt()),
        ollama::Message::user(prompt),
    ];

    let response = client.chat(model, messages, false).await?;
    let elapsed = start_time.elapsed();
    
    let plan = parse_plan(&response)?;
    
    println!("{} Reponse en {:.2}s", style("⏱").with(Color::DarkGrey), elapsed.as_secs_f64());

    if plan.changes.is_empty() {
        println!("{} Aucune modification proposee.", style("plan:").with(Color::Yellow));
        println!("{} {}", style("summary:").with(Color::Cyan), plan.summary);
        return Ok(());
    }

    print_plan(&plan, workspace_root)?;

    if !ask_confirmation()? {
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
    let json_text = extract_json_object(response).ok_or_else(|| {
        anyhow!("the model did not return valid JSON. Response:\n{}", 
            if response.len() > 500 {
                format!("{}...", &response[..500])
            } else {
                response.to_string()
            }
        )
    })?;
    let plan: EditPlan = serde_json::from_str(json_text).context(
        format!("failed to parse edit plan JSON:\n{}", json_text)
    )?;
    Ok(plan)
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in text[start..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
            }
            '{' if !in_string => {
                depth += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let end = start + i + 1;
                    return Some(&text[start..end]);
                }
            }
            _ => {}
        }
    }

    None
}

fn print_plan(plan: &EditPlan, workspace_root: &Path) -> Result<()> {
    println!();
    print_section_header("PROPOSED CHANGES");
    println!("{}", style(&plan.summary).with(Color::White));
    
    // Calculate statistics
    let mut creates = 0;
    let mut updates = 0;
    let mut deletes = 0;
    
    for change in &plan.changes {
        match change.action {
            ChangeAction::Create => creates += 1,
            ChangeAction::Update => updates += 1,
            ChangeAction::Delete => deletes += 1,
        }
    }
    
    // Print overview stats
    println!();
    println!(
        "{}  {} files to modify",
        style("Overview:").with(Color::Cyan).bold(),
        style(plan.changes.len().to_string()).with(Color::White).bold()
    );
    
    if creates > 0 {
        println!(
            "  {} {}  create",
            style("+").with(Color::Green).bold(),
            style(creates.to_string()).with(Color::Green)
        );
    }
    if updates > 0 {
        println!(
            "  {} {}  update",
            style("~").with(Color::Yellow).bold(),
            style(updates.to_string()).with(Color::Yellow)
        );
    }
    if deletes > 0 {
        println!(
            "  {} {}  delete",
            style("-").with(Color::Red).bold(),
            style(deletes.to_string()).with(Color::Red)
        );
    }
    
    println!();
    print_legend();
    print_thin_rule();

    for (idx, change) in plan.changes.iter().enumerate() {
        let path = normalize_path(workspace_root, &change.path);
        let (action_symbol, action_text, action_color) = match change.action {
            ChangeAction::Create => ("+", "CREATE", Color::Green),
            ChangeAction::Update => ("~", "UPDATE", Color::Yellow),
            ChangeAction::Delete => ("-", "DELETE", Color::Red),
        };

        println!();
        println!(
            "{} {} {}{}",
            style(action_symbol).with(action_color).bold(),
            style(format!("[{}/{}]", idx + 1, plan.changes.len())).with(Color::DarkGrey),
            style(action_text).with(action_color).bold(),
            style(format!("  {}", path.display())).with(Color::White)
        );
        
        if let Some(note) = &change.note {
            println!("    {}", style(&note).with(Color::DarkGrey));
        }

        println!();
        
        match change.action {
            ChangeAction::Delete => {
                if let Ok(existing) = fs::read_to_string(&path) {
                    let diff = similar::TextDiff::from_lines(existing.as_str(), "")
                        .unified_diff()
                        .header(&format!("a/{}", change.path), &format!("b/{}", change.path))
                        .context_radius(2)
                        .to_string();
                    
                    let (_additions, deletions) = count_diff_stats(&diff);
                    println!(
                        "{}  {} {} lines removed",
                        style("Diff:").with(Color::DarkGrey),
                        style("-").with(Color::Red),
                        style(deletions).with(Color::Red)
                    );
                    print_colored_diff(&diff);
                } else {
                    println!("{} file not found on disk.", style("⚠ warning:").with(Color::Yellow));
                }
            }
            ChangeAction::Create | ChangeAction::Update => {
                if let Some(content) = &change.content {
                    let existing = fs::read_to_string(&path).unwrap_or_default();
                    let diff = similar::TextDiff::from_lines(&existing, content)
                        .unified_diff()
                        .header(&format!("a/{}", change.path), &format!("b/{}", change.path))
                        .context_radius(2)
                        .to_string();
                    
                    let (additions, deletions) = count_diff_stats(&diff);
                    let diff_summary = if additions > 0 || deletions > 0 {
                        format!(
                            "{}  {} {} added  {} {} removed",
                            style("Diff:").with(Color::DarkGrey),
                            style("+").with(Color::Green),
                            style(additions).with(Color::Green),
                            style("-").with(Color::Red),
                            style(deletions).with(Color::Red)
                        )
                    } else {
                        format!("{}  (no changes)", style("Diff:").with(Color::DarkGrey))
                    };
                    println!("{}", diff_summary);
                    
                    if !diff.is_empty() {
                        print_colored_diff(&diff);
                    }
                } else {
                    println!("{} no content provided", style("⚠ warning:").with(Color::Yellow));
                }
            }
        }

        print_thin_rule();
    }

    Ok(())
}

fn ask_confirmation() -> Result<bool> {
    println!();
    print_section_header("REVIEW & CONFIRM");
    println!(
        "{}",
        style("Please review the changes above carefully.").with(Color::White)
    );
    println!();
    print!("{} ", style("Apply changes? [y]es / [n]o:").with(Color::Cyan).bold());
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes" | "o" | "oui"))
}

fn print_colored_diff(diff: &str) {
    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            println!("{}", style(format!("  {}", line)).with(Color::Cyan).dim());
        } else if line.starts_with("@@") {
            println!("{}", style(format!("  {}", line)).with(Color::Magenta));
        } else if line.starts_with('+') {
            println!("{}", style(format!("  {}", line)).on(Color::DarkGreen).with(Color::White));
        } else if line.starts_with('-') {
            println!("{}", style(format!("  {}", line)).on(Color::DarkRed).with(Color::White));
        } else {
            println!("{}", style(format!("  {}", line)).with(Color::DarkGrey));
        }
    }
}

fn count_diff_stats(diff: &str) -> (usize, usize) {
    let mut additions = 0;
    let mut deletions = 0;
    
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }
    
    (additions, deletions)
}

fn print_section_header(title: &str) {
    println!();
    println!("{}", style("═".repeat(60)).with(Color::Cyan));
    println!("{} {}", style("▶").with(Color::Cyan), style(title).with(Color::Cyan).bold());
    println!("{}", style("═".repeat(60)).with(Color::Cyan));
}

fn print_legend() {
    println!(
        "{}  {}  {}  {}  {}  {}",
        style("Legend:").with(Color::DarkGrey),
        style("+").with(Color::Green).bold(),
        style("add").with(Color::DarkGrey),
        style("-").with(Color::Red).bold(),
        style("remove").with(Color::DarkGrey),
        style("@@").with(Color::Magenta).bold()
    );
}

fn print_thin_rule() {
    println!("{}", style("─".repeat(60)).with(Color::DarkGrey));
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
