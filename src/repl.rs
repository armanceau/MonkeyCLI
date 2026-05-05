use std::io::{self, Write};

use anyhow::Result;
use crossterm::style::{style, Color, Stylize};

use crate::{ollama, prompts, Mode};

pub async fn start(client: &ollama::OllamaClient, model: String, mode: Mode) -> Result<()> {
    let mut model = model;
    let mut messages = vec![ollama::Message::system(prompts::system_prompt(mode))];

    println!(
        "{} {}",
        style("MonkeyCLI").with(Color::Cyan).bold(),
        style(format!("({:?})", mode)).with(Color::DarkGrey)
    );
    println!("Tape /help pour voir les commandes, /exit pour quitter.");

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
                println!("/clear           Efface l'historique");
                println!("/model <nom>     Change le modele actif");
                println!("/models          Liste les modeles locaux");
                println!("/exit            Quitte");
                continue;
            }
            "/clear" => {
                messages.truncate(1);
                println!("Historique nettoye.");
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
            "/models" => {
                let local_models = client.list_models().await?;
                if local_models.is_empty() {
                    println!("Aucun modele local trouve.");
                } else {
                    for local_model in local_models {
                        println!("- {local_model}");
                    }
                }
                continue;
            }
            _ => {}
        }

        messages.push(ollama::Message::user(line.to_string()));

        let answer = client.chat(&model, messages.clone(), true).await;
        match answer {
            Ok(response) => {
                println!();
                println!();
                messages.push(ollama::Message::assistant(response));
            }
            Err(error) => {
                println!("{} {}", style("error:").with(Color::Red), error);
            }
        }
    }

    Ok(())
}
