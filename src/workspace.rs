use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_FILES: usize = 24;
const MAX_BYTES: usize = 120_000;
const MAX_FILE_BYTES: usize = 20_000;

pub fn collect_workspace_context(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;

    let mut output = String::new();
    output.push_str(&format!("Workspace root: {}\n", root.display()));
    output.push_str("Use only these files as source of truth.\n\n");
    output.push_str("Files:\n");

    let mut bytes_used = 0usize;
    let mut included = 0usize;

    for path in files {
        if included >= MAX_FILES || bytes_used >= MAX_BYTES {
            break;
        }

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let trimmed = trim_text(&content, MAX_FILE_BYTES);
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        let section = format!("\n--- FILE: {}\n{}\n", relative, trimmed);
        bytes_used += section.len();
        included += 1;
        output.push_str(&section);
    }

    if included == 0 {
        output.push_str("\n(no readable text files found)\n");
    }

    Ok(output)
}

fn collect_files(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current).with_context(|| format!("failed to read directory {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if should_skip(&path, &file_name) {
            continue;
        }

        if path.is_dir() {
            collect_files(root, &path, files)?;
            continue;
        }

        if is_supported_text_file(&path) {
            files.push(path);
        }
    }

    files.sort_by(|left, right| {
        let left_rel = left.strip_prefix(root).unwrap_or(left);
        let right_rel = right.strip_prefix(root).unwrap_or(right);
        left_rel.cmp(right_rel)
    });

    Ok(())
}

fn should_skip(path: &Path, file_name: &str) -> bool {
    let skip_dir = matches!(file_name, ".git" | "target" | "node_modules" | "dist" | "build");
    if skip_dir {
        return true;
    }

    if path.is_file() {
        return matches!(file_name, "Cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock");
    }

    false
}

fn is_supported_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()).as_deref(),
        Some("rs") | Some("md") | Some("toml") | Some("json") | Some("txt") | Some("yaml") | Some("yml") | Some("js") | Some("ts") | Some("tsx") | Some("jsx") | Some("py") | Some("go") | Some("java") | Some("c") | Some("cpp") | Some("h") | Some("html") | Some("css") | Some("sh") | Some("ps1")
    )
}

fn trim_text(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut end = max_bytes;
    while !content.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}\n... [truncated] ...", &content[..end])
}
