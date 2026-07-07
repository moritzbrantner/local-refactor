use crate::conventions::{ConventionSettings, PartialConventionSettings};
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
    pub conventions: ConventionSettings,
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            rules: vec!["simplify-conditional".to_string()],
            protected_paths: default_protected_paths(),
            validation_commands: Vec::new(),
            test_file_mode: TestFileMode::ReadOnly,
            conventions: ConventionSettings::default(),
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
        if let Some(conventions) = layer.conventions {
            self.conventions.apply_partial(conventions);
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
    #[serde(default)]
    pub conventions: Option<PartialConventionSettings>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_uses_default_rule_and_read_only_tests() {
        let config = EffectiveConfig::default();

        assert_eq!(config.rules, vec!["simplify-conditional"]);
        assert_eq!(config.test_file_mode, TestFileMode::ReadOnly);
        assert!(config.conventions.typescript.formatter.enabled);
        assert!(config.protected_paths.contains(&"Cargo.lock".to_string()));
        assert!(config.protected_paths.contains(&"bun.lock".to_string()));
    }

    #[test]
    fn default_protected_paths_cover_common_build_outputs_and_lockfiles() {
        let protected = default_protected_paths();

        for expected in [
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
        ] {
            assert!(protected.contains(&expected.to_string()), "{expected}");
        }
    }

    #[test]
    fn config_layer_can_parse_conventions() {
        let layer: ConfigLayer = toml::from_str(
            r#"
            [conventions]
            profile = "custom"

            [conventions.typescript.formatter]
            enabled = false
            requireConfig = true

            [conventions.rust.ordering]
            useItems = false
            memberGroups = ["methods", "constants"]
            "#,
        )
        .unwrap();

        let mut config = EffectiveConfig::default();
        config.apply_layer(layer);

        assert!(!config.conventions.typescript.formatter.enabled);
        assert!(!config.conventions.rust.ordering.use_items);
        assert_eq!(
            config.conventions.rust.ordering.member_groups,
            vec!["methods".to_string(), "constants".to_string()]
        );
    }
}
