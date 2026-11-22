#![forbid(unsafe_code)]

mod build_info;
mod context;
mod error;
pub mod ops;
mod registry;
mod reporting;
mod service_config;

pub use build_info::BuildInfo;
pub use context::Context;
pub use error::{Error, Result};
pub use reporting::{
    ReportDiagnostic, ReportDocument, ReportKind, ReportStatus, ReportTrigger, ReportVerification,
    ReportingPreference, UpgradeRecommendation,
};
