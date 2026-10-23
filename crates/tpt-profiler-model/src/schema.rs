//! Versioned schema envelope for all persisted and emitted data
//! (spec §69).
//!
//! Strategy: every JSON artefact (run records, baselines, opportunity
//! state, reports) is wrapped in a [`SchemaEnvelope`] carrying
//! [`SCHEMA_VERSION`]. Readers reject data they cannot interpret —
//! [`SerializationError::SchemaTooNew`] or
//! [`SerializationError::SchemaTooOld`] — and never silently reinterpret
//! fields. Breaking format changes bump [`SCHEMA_VERSION`]; migration
//! paths (when added) read old versions explicitly.

use crate::error::SerializationError;
use alloc::string::String;

/// Current schema version for all persisted/emitted profiler data.
///
/// Bump on any breaking change to the JSON shape; see `DESIGN.md` §5.
pub const SCHEMA_VERSION: u32 = 1;

/// Oldest schema version this build can read.
pub const MIN_SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Envelope wrapping every persisted/emitted profiler artefact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
pub struct SchemaEnvelope<T> {
    /// Schema version of [`Self::data`].
    pub schema_version: u32,
    /// Tool version that produced this artefact (e.g. `tpt-profiler 0.1.0`).
    pub generated_by: String,
    /// The payload.
    pub data: T,
}

impl<T> SchemaEnvelope<T> {
    /// Wraps `data` in an envelope with the current [`SCHEMA_VERSION`].
    #[must_use]
    pub fn new(generated_by: impl Into<String>, data: T) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            generated_by: generated_by.into(),
            data,
        }
    }

    /// Checks that this envelope's version is readable by this build.
    ///
    /// Returns the payload version on success.
    pub fn validate(&self) -> Result<u32, SerializationError> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(SerializationError::SchemaTooNew {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        if self.schema_version < MIN_SUPPORTED_SCHEMA_VERSION {
            return Err(SerializationError::SchemaTooOld {
                found: self.schema_version,
                minimum: MIN_SUPPORTED_SCHEMA_VERSION,
            });
        }
        Ok(self.schema_version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_envelope_carries_current_schema_version() {
        let envelope = SchemaEnvelope::new("tpt-profiler 0.1.0", 42_u32);
        assert_eq!(envelope.schema_version, SCHEMA_VERSION);
        assert_eq!(envelope.validate(), Ok(SCHEMA_VERSION));
    }

    #[test]
    fn future_versions_are_rejected_as_too_new() {
        let envelope = SchemaEnvelope::<u8> {
            schema_version: SCHEMA_VERSION + 1,
            generated_by: String::from("test"),
            data: 0,
        };
        assert_eq!(
            envelope.validate(),
            Err(SerializationError::SchemaTooNew {
                found: SCHEMA_VERSION + 1,
                supported: SCHEMA_VERSION,
            })
        );
    }

    #[test]
    fn ancient_versions_are_rejected_as_too_old() {
        let envelope = SchemaEnvelope::<u8> {
            schema_version: MIN_SUPPORTED_SCHEMA_VERSION - 1,
            generated_by: String::from("test"),
            data: 0,
        };
        assert!(matches!(
            envelope.validate(),
            Err(SerializationError::SchemaTooOld { .. })
        ));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_is_stable() {
        let envelope = SchemaEnvelope::new("tpt-profiler 0.1.0", alloc::vec![String::from("a")]);
        let json = serde_json::to_string(&envelope).expect("encode");
        assert!(json.contains("\"schema_version\":1"));
        let decoded: SchemaEnvelope<Vec<String>> = serde_json::from_str(&json).expect("decode");
        assert_eq!(decoded, envelope);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn round_trips_through_json_value_form() {
        use alloc::format;
        let envelope = SchemaEnvelope::new("test", 7_u32);
        let json =
            format!("{{\"schema_version\":{SCHEMA_VERSION},\"generated_by\":\"test\",\"data\":7}}");
        let decoded: SchemaEnvelope<u32> = serde_json::from_str(&json).expect("decode");
        assert_eq!(decoded, envelope);
    }
}
