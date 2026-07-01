use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RunStatus {
    Queued,
    Preparing,
    Analyzing,
    Planning,
    Editing,
    Validating,
    Repairing,
    Succeeded,
    Failed,
    Cancelled,
    Reverted,
}

impl RunStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Analyzing => "analyzing",
            Self::Planning => "planning",
            Self::Editing => "editing",
            Self::Validating => "validating",
            Self::Repairing => "repairing",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Reverted => "reverted",
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RunStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "preparing" => Ok(Self::Preparing),
            "analyzing" => Ok(Self::Analyzing),
            "planning" => Ok(Self::Planning),
            "editing" => Ok(Self::Editing),
            "validating" => Ok(Self::Validating),
            "repairing" => Ok(Self::Repairing),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "reverted" => Ok(Self::Reverted),
            _ => Err(()),
        }
    }
}
