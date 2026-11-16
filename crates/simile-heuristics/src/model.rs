use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Controls how much provider-neutral inference Simile may use during synthesis.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    Off,
    Conservative,
    #[default]
    Standard,
    Aggressive,
}

impl Policy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Conservative => "conservative",
            Self::Standard => "standard",
            Self::Aggressive => "aggressive",
        }
    }

    pub(crate) const fn admits(self, confidence: Confidence) -> bool {
        match self {
            Self::Off => false,
            Self::Conservative => matches!(confidence, Confidence::High),
            Self::Standard => matches!(confidence, Confidence::High | Confidence::Medium),
            Self::Aggressive => true,
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Policy {
    type Err = ParsePolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "off" => Ok(Self::Off),
            "conservative" => Ok(Self::Conservative),
            "standard" => Ok(Self::Standard),
            "aggressive" => Ok(Self::Aggressive),
            _ => Err(ParsePolicyError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParsePolicyError;

impl fmt::Display for ParsePolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected off, conservative, standard, or aggressive")
    }
}

impl std::error::Error for ParsePolicyError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Rule {
    pub id: String,
    pub confidence: Confidence,
    pub evidence: Evidence,
    pub action: Action,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "paths")]
pub enum Evidence {
    ExactFields(Vec<String>),
}

impl Evidence {
    pub(crate) fn paths(&self) -> &[String] {
        match self {
            Self::ExactFields(paths) => paths,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Action {
    Ordered {
        earlier: String,
        later: String,
    },
    Count {
        collection: String,
        count: String,
    },
    Difference {
        minuend: String,
        subtrahend: String,
        result: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Application {
    pub rule_id: String,
    pub confidence: Confidence,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Change {
    pub path: String,
    pub before: Value,
    pub after: Value,
}
