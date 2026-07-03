use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestFileMode {
    #[default]
    ReadOnly,
    Mutable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfig {
    pub rules: Vec<String>,
    pub protected_paths: Vec<String>,
    pub validation_commands: Vec<String>,
    pub test_file_mode: TestFileMode,
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            rules: vec!["simplify-conditional".to_string()],
            protected_paths: default_protected_paths(),
            validation_commands: Vec::new(),
            test_file_mode: TestFileMode::ReadOnly,
        }
    }
}

impl EffectiveConfig {
    pub fn apply_layer(&mut self, layer: ConfigLayer) {
        if let Some(rules) = layer.rules {
            self.rules = rules;
        }
        if let Some(protected_paths) = layer.protected_paths {
            self.protected_paths.extend(protected_paths);
        }
        if let Some(validation_commands) = layer.validation_commands {
            self.validation_commands = validation_commands;
        }
        if let Some(test_file_mode) = layer.test_file_mode {
            self.test_file_mode = test_file_mode;
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigLayer {
    #[serde(default)]
    pub rules: Option<Vec<String>>,
    #[serde(default)]
    pub protected_paths: Option<Vec<String>>,
    #[serde(default)]
    pub validation_commands: Option<Vec<String>>,
    #[serde(default)]
    pub test_file_mode: Option<TestFileMode>,
}

pub fn load_config_file(path: &Path) -> Result<ConfigLayer> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    toml::from_str(&contents).with_context(|| format!("invalid TOML in {}", path.display()))
}

pub fn find_project_config(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent().map(Path::to_path_buf)
    } else {
        Some(start.to_path_buf())
    };

    while let Some(dir) = current {
        let candidate = dir.join("refactor-rules.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        current = dir.parent().map(Path::to_path_buf);
    }

    None
}

pub fn default_protected_paths() -> Vec<String> {
    vec![
        "node_modules/**",
        "dist/**",
        "build/**",
        ".next/**",
        "coverage/**",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "Cargo.lock",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
