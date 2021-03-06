// Re-exported as almost every crate using data will also need decimal and
// NaiveDate
pub use chrono;
pub use rust_decimal;
mod datatype;
mod datum;
pub mod encoding_core;
mod encoding_datum;
pub mod json;
mod json_serde;
mod session;
mod tuple_iter;
pub use datatype::*;
pub use datum::Datum;
use serde::export::Formatter;
pub use session::Session;
use std::fmt::Display;
pub use tuple_iter::*;
pub mod jsonpath_utils;

#[macro_use]
extern crate lazy_static;

/// General sort order enum.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn is_asc(&self) -> bool {
        *self == SortOrder::Asc
    }

    pub fn is_desc(&self) -> bool {
        *self == SortOrder::Desc
    }
}

impl Display for SortOrder {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SortOrder::Asc => f.write_str("ASC"),
            SortOrder::Desc => f.write_str("DESC"),
        }
    }
}

/// Timestamps for tracking tuples through the system, used for MVCC style point in time queries,
#[derive(Default, Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd)]
pub struct LogicalTimestamp {
    pub ms: u64,
}

impl LogicalTimestamp {
    pub const MAX: LogicalTimestamp = LogicalTimestamp { ms: u64::MAX };
    /// Creates a new Logical timestamp based on the number of ms since 1970.
    pub fn new(ms: u64) -> Self {
        LogicalTimestamp { ms }
    }

    /// Creates a new Logical timestamp based on the current system time.
    pub fn now() -> Self {
        LogicalTimestamp {
            ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }
}
