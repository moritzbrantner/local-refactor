use crate::{
    analyzer::AnalyzerEdit, request_target_path, run_intake::validation_root, RunCreateRequest,
};
use anyhow::{anyhow, Context, Result};
use local_refactor_core::conventions::ConventionSettings;
use std::path::{Path, PathBuf};
use tokio::{
    io::AsyncWriteExt,
    process::{ChildStdin, Command},
};

pub(crate) async fn plan_rule(
    request: &RunCreateRequest,
    rule_id: &str,
    files: &[String],
    current_content: &mut impl FnMut(&str) -> Result<String>,
) -> Result<(Vec<AnalyzerEdit>, Vec<String>)> {
    let conventions = request.convention_snapshot.clone().unwrap_or_default();
    let root = repository_root(request)?;
    let mut edits = Vec::new();
    let mut diagnostics = Vec::new();

    for file in files {
        let original_content = current_content(file)?;
        let planned = match rule_id {
            "format-typescript" => {
                format_typescript(&root, file, &original_content, &conventions).await?
            }
            "format-rust" => format_rust(&root, file, &original_content, &conventions).await?,
            "sort-rust-use-items" => sort_rust_use_items(file, &original_content, &conventions),
            "sort-rust-impl-members" => {
                sort_rust_impl_members(file, &original_content, &conventions)
            }
            _ => return Err(anyhow!("unsupported convention rule: {rule_id}")),
        };
        diagnostics.extend(planned.diagnostics);
        if planned.content != original_content {
            edits.push(AnalyzerEdit {
                file_path: file.clone(),
                original_content,
                new_content: planned.content,
                rule_id: rule_id.to_string(),
                summary: planned.summary,
            });
        }
    }

    Ok((edits, diagnostics))
}

pub(crate) fn is_service_convention_rule(rule_id: &str) -> bool {
    matches!(
        rule_id,
        "format-typescript" | "format-rust" | "sort-rust-use-items" | "sort-rust-impl-members"
    )
}

struct PlannedContent {
    content: String,
    summary: String,
    diagnostics: Vec<String>,
}

async fn format_typescript(
    root: &Path,
    file: &str,
    content: &str,
    conventions: &ConventionSettings,
) -> Result<PlannedContent> {
    let settings = &conventions.typescript.formatter;
    if !settings.enabled {
        return Ok(no_edit(
            content,
            format!("Skipped {file}: TypeScript formatter disabled"),
        ));
    }
    let config = find_prettier_config(root);
    if settings.require_config && config.is_none() {
        return Err(anyhow!(
            "format-typescript requires a Prettier config for {}",
            root.display()
        ));
    }
    let command = find_prettier_command(root).ok_or_else(|| {
        anyhow!("format-typescript requires prettier in node_modules/.bin or on PATH")
    })?;
    let mut args = vec!["--stdin-filepath".to_string(), file.to_string()];
    if let Some(config) = config {
        args.push("--config".to_string());
        args.push(config.to_string_lossy().to_string());
    }
    let formatted = run_formatter(command, &args, content).await?;
    Ok(PlannedContent {
        content: formatted,
        summary: "Formatted TypeScript with Prettier".to_string(),
        diagnostics: vec![format!("Formatted {file} with Prettier")],
    })
}

async fn format_rust(
    root: &Path,
    file: &str,
    content: &str,
    conventions: &ConventionSettings,
) -> Result<PlannedContent> {
    let settings = &conventions.rust.formatter;
    if !settings.enabled {
        return Ok(no_edit(
            content,
            format!("Skipped {file}: Rust formatter disabled"),
        ));
    }
    let config = find_rustfmt_config(root);
    if settings.require_config && config.is_none() {
        return Err(anyhow!(
            "format-rust requires rustfmt.toml or .rustfmt.toml for {}",
            root.display()
        ));
    }
    let command = find_command_on_path("rustfmt")
        .ok_or_else(|| anyhow!("format-rust requires rustfmt on PATH"))?;
    let mut args = vec!["--emit".to_string(), "stdout".to_string()];
    if let Some(config) = config {
        args.push("--config-path".to_string());
        args.push(config.to_string_lossy().to_string());
    }
    args.push("--edition".to_string());
    args.push("2021".to_string());
    let formatted = run_formatter(command, &args, content).await?;
    Ok(PlannedContent {
        content: formatted,
        summary: "Formatted Rust with rustfmt".to_string(),
        diagnostics: vec![format!("Formatted {file} with rustfmt")],
    })
}

fn sort_rust_use_items(
    file: &str,
    content: &str,
    conventions: &ConventionSettings,
) -> PlannedContent {
    if !conventions.rust.ordering.use_items {
        return no_edit(
            content,
            format!("Skipped {file}: Rust use ordering disabled"),
        );
    }
    if let Some(reason) = rust_structural_skip_reason(content) {
        return no_edit(content, format!("Skipped {file}: {reason}"));
    }

    let mut lines = content.split('\n').map(str::to_string).collect::<Vec<_>>();
    let mut changed = false;
    let mut index = 0;
    while index < lines.len() {
        if !is_simple_use_line(&lines[index]) {
            index += 1;
            continue;
        }
        let start = index;
        while index < lines.len() && is_simple_use_line(&lines[index]) {
            index += 1;
        }
        let end = index;
        if end - start < 2 {
            continue;
        }
        let sorted = {
            let mut block = lines[start..end].to_vec();
            block.sort_by_key(|line| line.trim().to_string());
            block
        };
        if sorted != lines[start..end] {
            lines.splice(start..end, sorted);
            changed = true;
        }
    }

    PlannedContent {
        content: if changed {
            lines.join("\n")
        } else {
            content.to_string()
        },
        summary: "Sorted Rust use items".to_string(),
        diagnostics: vec![format!("Analyzed {file} for Rust use ordering")],
    }
}

fn sort_rust_impl_members(
    file: &str,
    content: &str,
    conventions: &ConventionSettings,
) -> PlannedContent {
    if !conventions.rust.ordering.impl_members {
        return no_edit(
            content,
            format!("Skipped {file}: Rust impl member ordering disabled"),
        );
    }
    if let Some(reason) = rust_structural_skip_reason(content) {
        return no_edit(content, format!("Skipped {file}: {reason}"));
    }

    let lines = content.split('\n').map(str::to_string).collect::<Vec<_>>();
    let Some((start, end)) = single_simple_impl_body(&lines) else {
        return no_edit(
            content,
            format!("Skipped {file}: no single simple impl body found"),
        );
    };
    let members = lines[start..end]
        .iter()
        .enumerate()
        .filter_map(|(offset, line)| {
            rust_impl_member_sort_key(line).map(|key| (offset, key, line.clone()))
        })
        .collect::<Vec<_>>();
    if members.len() < 2 || members.len() != end - start {
        return no_edit(
            content,
            format!("Skipped {file}: impl members are not all simple one-line items"),
        );
    }

    let mut sorted = members.clone();
    sorted.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.2.cmp(&right.2)));
    if sorted
        .iter()
        .zip(members.iter())
        .all(|(left, right)| left.2 == right.2)
    {
        return PlannedContent {
            content: content.to_string(),
            summary: "Sorted Rust impl members".to_string(),
            diagnostics: vec![format!("Analyzed {file} for Rust impl member ordering")],
        };
    }

    let mut next = lines;
    for (offset, (_, _, line)) in sorted.into_iter().enumerate() {
        next[start + offset] = line;
    }
    PlannedContent {
        content: next.join("\n"),
        summary: "Sorted Rust impl members".to_string(),
        diagnostics: vec![format!("Analyzed {file} for Rust impl member ordering")],
    }
}

async fn run_formatter(command: PathBuf, args: &[String], content: &str) -> Result<String> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| "failed to start formatter")?;
    write_stdin(child.stdin.take(), content).await?;
    let output = child.wait_with_output().await?;
    if !output.status.success() {
        return Err(anyhow!(
            "formatter failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(Into::into)
}

async fn write_stdin(stdin: Option<ChildStdin>, content: &str) -> Result<()> {
    let mut stdin = stdin.ok_or_else(|| anyhow!("formatter stdin unavailable"))?;
    stdin.write_all(content.as_bytes()).await?;
    stdin.shutdown().await?;
    Ok(())
}

fn rust_structural_skip_reason(content: &str) -> Option<String> {
    if content.contains("macro_rules!") || content.lines().any(|line| line.trim().contains("!(")) {
        return Some("macros are present".to_string());
    }
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Some("Rust parser could not be initialized".to_string());
    }
    let tree = parser.parse(content, None)?;
    if tree.root_node().has_error() {
        return Some("Rust parse errors are present".to_string());
    }
    None
}

fn is_simple_use_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("use ") && trimmed.ends_with(';') && !trimmed.contains('{')
}

fn single_simple_impl_body(lines: &[String]) -> Option<(usize, usize)> {
    let impl_start = lines.iter().position(|line| {
        line.trim_start().starts_with("impl ") && line.trim_end().ends_with('{')
    })?;
    let impl_end = lines
        .iter()
        .enumerate()
        .skip(impl_start + 1)
        .find(|(_, line)| line.trim() == "}")?
        .0;
    Some((impl_start + 1, impl_end))
}

fn rust_impl_member_sort_key(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim();
    let group = if trimmed.starts_with("type ") || trimmed.starts_with("pub type ") {
        0
    } else if trimmed.starts_with("const ") || trimmed.starts_with("pub const ") {
        1
    } else if trimmed.starts_with("fn new(") || trimmed.starts_with("pub fn new(") {
        2
    } else if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
        3
    } else {
        return None;
    };
    Some((group, trimmed.to_string()))
}

fn no_edit(content: &str, diagnostic: String) -> PlannedContent {
    PlannedContent {
        content: content.to_string(),
        summary: String::new(),
        diagnostics: vec![diagnostic],
    }
}

fn repository_root(request: &RunCreateRequest) -> Result<PathBuf> {
    if let Some(root) = request.repository_root_path.as_deref() {
        return Ok(PathBuf::from(root));
    }
    Ok(validation_root(request_target_path(request)?))
}

fn find_prettier_config(root: &Path) -> Option<PathBuf> {
    [
        ".prettierrc",
        ".prettierrc.json",
        ".prettierrc.yml",
        ".prettierrc.yaml",
        ".prettierrc.toml",
        "prettier.config.js",
        "prettier.config.cjs",
        "prettier.config.mjs",
        "prettier.config.ts",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .find(|path| path.exists())
}

fn find_prettier_command(root: &Path) -> Option<PathBuf> {
    let local = root.join("node_modules/.bin/prettier");
    if local.exists() {
        return Some(local);
    }
    find_command_on_path("prettier")
}

fn find_rustfmt_config(root: &Path) -> Option<PathBuf> {
    ["rustfmt.toml", ".rustfmt.toml"]
        .into_iter()
        .map(|name| root.join(name))
        .find(|path| path.exists())
}

fn find_command_on_path(command: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|path| path.join(command))
        .find(|path| path.exists())
}
