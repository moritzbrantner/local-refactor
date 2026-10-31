use super::PatchPlanModelRequest;
use anyhow::{anyhow, Result};
use local_refactor_core::rules::Language;

pub(super) fn validate_documentation_only_update(
    request: &PatchPlanModelRequest,
    path: &str,
    original: &str,
    updated: &str,
) -> Result<()> {
    if !is_documentation_only_rule(&request.rule_id) {
        return Ok(());
    }

    if request.language == Language::TypeScript {
        for directive in ["@ts-nocheck", "@ts-check", "@ts-ignore", "@ts-expect-error"] {
            if updated.contains(directive) && !original.contains(directive) {
                return Err(anyhow!(
                    "{}: documentation-only patch added forbidden TypeScript directive comment {}",
                    path,
                    directive
                ));
            }
        }
    }

    let original_code = code_without_comments(request.language, original);
    let updated_code = code_without_comments(request.language, updated);
    if !original_code
        .split_whitespace()
        .eq(updated_code.split_whitespace())
    {
        return Err(anyhow!(
            "{}: documentation-only patch changed non-comment code",
            path
        ));
    }

    Ok(())
}

pub(super) fn is_documentation_only_rule(rule_id: &str) -> bool {
    matches!(
        rule_id,
        "add-documentation-comments" | "rust-add-documentation-comments"
    )
}

fn code_without_comments(language: Language, value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '/' if chars.peek() == Some(&'/') => {
                chars.next();
                for comment_character in chars.by_ref() {
                    if comment_character == '\n' {
                        output.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for comment_character in chars.by_ref() {
                    if previous == '*' && comment_character == '/' {
                        break;
                    }
                    if comment_character == '\n' {
                        output.push('\n');
                    }
                    previous = comment_character;
                }
            }
            '"' => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '"');
            }
            '\'' if language == Language::TypeScript => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '\'');
            }
            '\'' if language == Language::Rust => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '\'');
            }
            '`' if language == Language::TypeScript => {
                output.push(character);
                copy_quoted(&mut chars, &mut output, '`');
            }
            _ => output.push(character),
        }
    }
    output
}

fn copy_quoted(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    output: &mut String,
    quote: char,
) {
    let mut escaped = false;
    for character in chars.by_ref() {
        output.push(character);
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            break;
        }
    }
}
