# MonkeyCLI

MonkeyCLI est une CLI connectée à Ollama pour piloter des modèles locaux en mode assistant, code et agent.
Le mode par defaut est le mode agent: il propose des modifications de fichiers, affiche le diff, puis attend une confirmation avant ecriture.

## Fonctionnalites

- Mode agent avec proposition de patch et confirmation
- Chat one-shot
- Mode code one-shot
- Sessions interactives (`repl`, `code-repl`)
- Liste des modeles locaux (`models`)
- Diagnostic de connexion (`doctor`)

## Prerequis

- Rust et Cargo
- Ollama lance en local
- Au moins un modèle installé

## Installation

```bash
cargo install --path . --force
```

Si la commande n'est pas reconnue sur Windows dans la session en cours:

```powershell
$env:Path += ";$env:USERPROFILE\.cargo\bin"
```

## Utilisation

```bash
monkeycli
monkeycli chat "explique REST vs GraphQL"
monkeycli code "ecris une fonction Rust pour parser un JSON"
monkeycli repl
monkeycli code-repl
monkeycli models
monkeycli doctor
```

## Modes

- `monkeycli`: mode agent (par defaut)
- `monkeycli chat`: reponse texte one-shot
- `monkeycli code`: reponse orientee code one-shot
- `monkeycli repl`: session interactive assistant
- `monkeycli code-repl`: session interactive orientee code

## Options globales

- `-m`, `--model <nom>`: force un modele
- `--host <url>`: change l'endpoint Ollama
- `--no-stream`: desactive l'affichage token par token

## Variables d'environnement

- `OLLAMA_HOST` (defaut: `http://127.0.0.1:11434`)
- `OLLAMA_MODEL` (modele par defaut)

## Commandes REPL

- `/help`
- `/clear`
- `/model <nom>`
- `/models`
- `/exit`
