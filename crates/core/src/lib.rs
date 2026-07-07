pub mod config;
pub mod conventions;
pub mod coverage;
pub mod path_policy;
pub mod rule_selection;
pub mod rules;

pub use config::{EffectiveConfig, TestFileMode};
pub use conventions::{ConventionSettings, PartialConventionSettings};
pub use coverage::{BehaviorClaim, CoverageEvidenceItem};
pub use path_policy::{PathDecision, PathPolicy};
pub use rule_selection::{RuleSelectionPlan, RuleSelectionReason, RuleSelectionSegment};
