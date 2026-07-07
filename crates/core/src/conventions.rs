use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConventionSettings {
    pub profile: ConventionProfile,
    pub typescript: TypeScriptConventionSettings,
    pub rust: RustConventionSettings,
}

impl Default for ConventionSettings {
    fn default() -> Self {
        Self {
            profile: ConventionProfile::Standard,
            typescript: TypeScriptConventionSettings::default(),
            rust: RustConventionSettings::default(),
        }
    }
}

impl ConventionSettings {
    pub fn apply_partial(&mut self, partial: PartialConventionSettings) {
        if let Some(profile) = partial.profile {
            self.profile = profile;
        }
        if let Some(typescript) = partial.typescript {
            self.typescript.apply_partial(typescript);
        }
        if let Some(rust) = partial.rust {
            self.rust.apply_partial(rust);
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConventionProfile {
    Standard,
    Minimal,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TypeScriptConventionSettings {
    pub formatter: FormatterConventionSettings,
    pub ordering: TypeScriptOrderingSettings,
}

impl Default for TypeScriptConventionSettings {
    fn default() -> Self {
        Self {
            formatter: FormatterConventionSettings::default(),
            ordering: TypeScriptOrderingSettings::default(),
        }
    }
}

impl TypeScriptConventionSettings {
    fn apply_partial(&mut self, partial: PartialTypeScriptConventionSettings) {
        if let Some(formatter) = partial.formatter {
            self.formatter.apply_partial(formatter);
        }
        if let Some(ordering) = partial.ordering {
            self.ordering.apply_partial(ordering);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RustConventionSettings {
    pub formatter: FormatterConventionSettings,
    pub ordering: RustOrderingSettings,
}

impl Default for RustConventionSettings {
    fn default() -> Self {
        Self {
            formatter: FormatterConventionSettings::default(),
            ordering: RustOrderingSettings::default(),
        }
    }
}

impl RustConventionSettings {
    fn apply_partial(&mut self, partial: PartialRustConventionSettings) {
        if let Some(formatter) = partial.formatter {
            self.formatter.apply_partial(formatter);
        }
        if let Some(ordering) = partial.ordering {
            self.ordering.apply_partial(ordering);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormatterConventionSettings {
    pub enabled: bool,
    pub require_config: bool,
}

impl Default for FormatterConventionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            require_config: true,
        }
    }
}

impl FormatterConventionSettings {
    fn apply_partial(&mut self, partial: PartialFormatterConventionSettings) {
        if let Some(enabled) = partial.enabled {
            self.enabled = enabled;
        }
        if let Some(require_config) = partial.require_config {
            self.require_config = require_config;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TypeScriptOrderingSettings {
    pub imports: bool,
    pub class_members: bool,
    pub member_groups: Vec<String>,
    pub alphabetical_within_groups: bool,
}

impl Default for TypeScriptOrderingSettings {
    fn default() -> Self {
        Self {
            imports: true,
            class_members: true,
            member_groups: vec![
                "static-fields".to_string(),
                "fields".to_string(),
                "constructors".to_string(),
                "methods".to_string(),
            ],
            alphabetical_within_groups: true,
        }
    }
}

impl TypeScriptOrderingSettings {
    fn apply_partial(&mut self, partial: PartialTypeScriptOrderingSettings) {
        if let Some(imports) = partial.imports {
            self.imports = imports;
        }
        if let Some(class_members) = partial.class_members {
            self.class_members = class_members;
        }
        if let Some(member_groups) = partial.member_groups {
            self.member_groups = member_groups;
        }
        if let Some(alphabetical_within_groups) = partial.alphabetical_within_groups {
            self.alphabetical_within_groups = alphabetical_within_groups;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RustOrderingSettings {
    pub use_items: bool,
    pub impl_members: bool,
    pub member_groups: Vec<String>,
    pub alphabetical_within_groups: bool,
}

impl Default for RustOrderingSettings {
    fn default() -> Self {
        Self {
            use_items: true,
            impl_members: true,
            member_groups: vec![
                "associated-types".to_string(),
                "constants".to_string(),
                "constructors".to_string(),
                "methods".to_string(),
            ],
            alphabetical_within_groups: true,
        }
    }
}

impl RustOrderingSettings {
    fn apply_partial(&mut self, partial: PartialRustOrderingSettings) {
        if let Some(use_items) = partial.use_items {
            self.use_items = use_items;
        }
        if let Some(impl_members) = partial.impl_members {
            self.impl_members = impl_members;
        }
        if let Some(member_groups) = partial.member_groups {
            self.member_groups = member_groups;
        }
        if let Some(alphabetical_within_groups) = partial.alphabetical_within_groups {
            self.alphabetical_within_groups = alphabetical_within_groups;
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartialConventionSettings {
    #[serde(default)]
    pub profile: Option<ConventionProfile>,
    #[serde(default)]
    pub typescript: Option<PartialTypeScriptConventionSettings>,
    #[serde(default)]
    pub rust: Option<PartialRustConventionSettings>,
}

impl PartialConventionSettings {
    pub fn is_empty(&self) -> bool {
        self.profile.is_none() && self.typescript.is_none() && self.rust.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartialTypeScriptConventionSettings {
    #[serde(default)]
    pub formatter: Option<PartialFormatterConventionSettings>,
    #[serde(default)]
    pub ordering: Option<PartialTypeScriptOrderingSettings>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartialRustConventionSettings {
    #[serde(default)]
    pub formatter: Option<PartialFormatterConventionSettings>,
    #[serde(default)]
    pub ordering: Option<PartialRustOrderingSettings>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartialFormatterConventionSettings {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub require_config: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartialTypeScriptOrderingSettings {
    #[serde(default)]
    pub imports: Option<bool>,
    #[serde(default)]
    pub class_members: Option<bool>,
    #[serde(default)]
    pub member_groups: Option<Vec<String>>,
    #[serde(default)]
    pub alphabetical_within_groups: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartialRustOrderingSettings {
    #[serde(default)]
    pub use_items: Option<bool>,
    #[serde(default)]
    pub impl_members: Option<bool>,
    #[serde(default)]
    pub member_groups: Option<Vec<String>>,
    #[serde(default)]
    pub alphabetical_within_groups: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_settings_override_nested_defaults() {
        let mut settings = ConventionSettings::default();

        settings.apply_partial(PartialConventionSettings {
            profile: Some(ConventionProfile::Custom),
            typescript: Some(PartialTypeScriptConventionSettings {
                formatter: Some(PartialFormatterConventionSettings {
                    enabled: Some(false),
                    require_config: None,
                }),
                ordering: Some(PartialTypeScriptOrderingSettings {
                    imports: Some(false),
                    class_members: None,
                    member_groups: Some(vec!["methods".to_string(), "fields".to_string()]),
                    alphabetical_within_groups: None,
                }),
            }),
            rust: None,
        });

        assert_eq!(settings.profile, ConventionProfile::Custom);
        assert!(!settings.typescript.formatter.enabled);
        assert!(settings.typescript.formatter.require_config);
        assert!(!settings.typescript.ordering.imports);
        assert_eq!(
            settings.typescript.ordering.member_groups,
            vec!["methods".to_string(), "fields".to_string()]
        );
        assert!(settings.rust.formatter.enabled);
    }
}
