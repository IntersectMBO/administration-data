//! Chain timestamp helpers
//!
//! On-chain block times are returned as a paired `{unix, iso}` object so
//! clients don't have to choose between Unix-int sortability and
//! ISO-8601 readability. Server-side timestamps (`created_at`, etc.) keep
//! their plain `DateTime<Utc>` representation.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// On-chain block timestamp, paired as Unix seconds and ISO 8601 UTC.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct ChainTime {
    /// POSIX seconds since 1970-01-01.
    pub unix: i64,
    /// ISO 8601 UTC string for display.
    pub iso: DateTime<Utc>,
}

impl ChainTime {
    pub fn from_secs(secs: i64) -> Self {
        let iso = Utc.timestamp_opt(secs, 0).single().unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap());
        Self { unix: secs, iso }
    }

    pub fn maybe_from_secs(opt: Option<i64>) -> Option<Self> {
        opt.map(Self::from_secs)
    }
}
