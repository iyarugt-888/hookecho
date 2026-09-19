//! Source-independent provenance for weather fields. Unknown times remain unknown.
mod descriptor;
mod grid;
use chrono::{DateTime, Duration, Utc};
pub use descriptor::{
    DataSource, FieldDescriptor, FieldFamily, FieldId, GeographicBounds, PaletteId, Unit, ValueKind,
};
pub use grid::{DisplayTransform, GridGeometry, GridProvenance};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualitySummary {
    /// The provider's quality flags have not been decoded.
    Unknown,
    Good,
    Suspect(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataStamp {
    pub source_id: String,
    pub product_id: String,
    pub issue_time: Option<DateTime<Utc>>,
    pub run_time: Option<DateTime<Utc>>,
    pub valid_time: DateTime<Utc>,
    /// Local payload arrival time; when the fetcher cannot capture body arrival separately,
    /// this is decode completion time. Never inferred from a forecast's valid time.
    pub received_time: DateTime<Utc>,
    /// Provider ingest latency, only when independently supplied by the provider.
    pub source_latency: Option<Duration>,
    pub is_forecast: bool,
    pub is_derived: bool,
    pub quality: QualitySummary,
    #[serde(default)]
    pub grid: Option<GridProvenance>,
}

impl DataStamp {
    /// Signed: future valid times must not masquerade as fresh observations.
    pub fn age_at(&self, now: DateTime<Utc>) -> Duration {
        now - self.valid_time
    }

    pub fn receipt_age_at(&self, now: DateTime<Utc>) -> Duration {
        now - self.received_time
    }
}

/// Keeps provenance with a payload without changing existing renderer/grid APIs.
#[derive(Clone)]
pub struct Stamped<T> {
    pub data: T,
    pub stamp: DataStamp,
}

impl<T> Stamped<T> {
    /// Display transformations retain the original source clock and identity.
    pub fn map<U>(self, transform: impl FnOnce(T) -> U) -> Stamped<U> {
        Stamped {
            data: transform(self.data),
            stamp: self.stamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_and_serialization_preserve_forecast_and_unknown_metadata() {
        let valid = DateTime::from_timestamp(1_000, 0).unwrap();
        let stamp = DataStamp {
            source_id: "test".into(),
            product_id: "forecast".into(),
            issue_time: None,
            run_time: None,
            valid_time: valid,
            received_time: valid - Duration::seconds(30),
            source_latency: None,
            is_forecast: true,
            is_derived: false,
            quality: QualitySummary::Unknown,
            grid: None,
        };
        assert_eq!(
            stamp.age_at(valid - Duration::seconds(10)).num_seconds(),
            -10
        );
        assert_eq!(stamp.receipt_age_at(valid).num_seconds(), 30);
        let json = serde_json::to_string(&stamp).unwrap();
        assert_eq!(serde_json::from_str::<DataStamp>(&json).unwrap(), stamp);
        let transformed = Stamped {
            data: 4,
            stamp: stamp.clone(),
        }
        .map(|v| v / 2);
        assert_eq!(transformed.data, 2);
        assert_eq!(transformed.stamp, stamp);
    }
}
