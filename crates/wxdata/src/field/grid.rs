//! Grid provenance survives texture-size reduction independently of pixel values.
use super::{Stamped, ValueKind};
use crate::mrms::MrmsField;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridGeometry {
    pub nx: usize,
    pub ny: usize,
    /// Bounds carried by the regular longitude/latitude grid: west, south, east, north.
    pub bounds: [f64; 4],
}

impl From<&MrmsField> for GridGeometry {
    fn from(grid: &MrmsField) -> Self {
        Self {
            nx: grid.nx,
            ny: grid.ny,
            bounds: [grid.lon_west, grid.lat_south, grid.lon_east, grid.lat_north],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayTransform {
    Native,
    MaximumPool { factor: usize },
    NearestCell,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridProvenance {
    pub native: GridGeometry,
    pub displayed: GridGeometry,
    pub transform: DisplayTransform,
    /// Set when the values are interpolated in time between two real frames rather than read
    /// from one (1008.md E2); a probe or export of such a frame says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<TimeBlend>,
}

impl GridProvenance {
    pub fn native(grid: &MrmsField) -> Self {
        Self {
            native: grid.into(),
            displayed: grid.into(),
            transform: DisplayTransform::Native,
            blend: None,
        }
    }
}

/// The two real frames an interpolated one lies between, and how far along.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeBlend {
    pub before: chrono::DateTime<chrono::Utc>,
    pub after: chrono::DateTime<chrono::Utc>,
    /// 0 is the earlier frame, 1 the later.
    pub weight_after: f64,
}

/// Why two frames are not blended.
#[derive(Debug, Clone, PartialEq)]
pub enum BlendRefused {
    /// Categories, masks, vectors and accumulations are never interpolated in time: a value
    /// halfway between two classes or two accumulation windows means nothing.
    Kind(ValueKind),
    /// Different products, sources or runs.
    Product,
    /// Different grids: cells would not be the same place.
    Grid,
    /// The instant is not strictly between the two frames.
    Order,
}

impl std::fmt::Display for BlendRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlendRefused::Kind(kind) => write!(f, "{kind:?} values are not interpolated in time"),
            BlendRefused::Product => write!(f, "the frames are different products or runs"),
            BlendRefused::Grid => write!(f, "the frames are on different grids"),
            BlendRefused::Order => write!(f, "the instant is not between the two frames"),
        }
    }
}

/// The field at `at`, interpolated linearly in time between two real frames of the same product
/// on the same grid (1008.md E2). Only scalar and probability values are blended. A cell missing
/// in either frame is missing in the blend: no value is invented where one frame has none (an
/// echo appearing between scans pops in at the later frame rather than fading from nothing).
/// The result carries [`TimeBlend`] in its grid provenance and is marked derived.
pub fn blend_frames(
    before: &Stamped<MrmsField>,
    after: &Stamped<MrmsField>,
    at: chrono::DateTime<chrono::Utc>,
    kind: ValueKind,
) -> Result<Stamped<MrmsField>, BlendRefused> {
    if !matches!(kind, ValueKind::Scalar | ValueKind::Probability) {
        return Err(BlendRefused::Kind(kind));
    }
    let (a, b) = (&before.stamp, &after.stamp);
    if a.product_id != b.product_id
        || a.source_id != b.source_id
        || a.run_time != b.run_time
        || a.is_forecast != b.is_forecast
    {
        return Err(BlendRefused::Product);
    }
    if GridGeometry::from(&before.data) != GridGeometry::from(&after.data)
        || before.data.values.len() != after.data.values.len()
    {
        return Err(BlendRefused::Grid);
    }
    let (t0, t1) = (a.valid_time, b.valid_time);
    if !(t0 < at && at < t1) {
        return Err(BlendRefused::Order);
    }
    let w = (at - t0).num_milliseconds() as f64 / (t1 - t0).num_milliseconds() as f64;
    let wf = w as f32;
    let values = before
        .data
        .values
        .iter()
        .zip(&after.data.values)
        .map(|(&x, &y)| {
            if x.is_finite() && y.is_finite() {
                x + (y - x) * wf
            } else {
                f32::NAN
            }
        })
        .collect();
    let data = MrmsField {
        values,
        time: at,
        ..before.data.clone()
    };
    let mut grid = GridProvenance::native(&data);
    grid.blend = Some(TimeBlend {
        before: t0,
        after: t1,
        weight_after: w,
    });
    let stamp = super::DataStamp {
        valid_time: at,
        received_time: a.received_time.max(b.received_time),
        is_derived: true,
        grid: Some(grid),
        ..a.clone()
    };
    Ok(Stamped { data, stamp })
}

impl Stamped<MrmsField> {
    /// Reduce a freshly decoded field for display. Categories are sampled, never max-pooled.
    /// Native geometry is retained; this does not retain the native value array for probing.
    pub fn for_display(mut self, max_dim: usize, kind: ValueKind) -> Self {
        let factor = self
            .data
            .nx
            .max(self.data.ny)
            .div_ceil(max_dim.max(1))
            .max(1);
        let metadata = self
            .stamp
            .grid
            .get_or_insert_with(|| GridProvenance::native(&self.data));
        if factor > 1 {
            if matches!(kind, ValueKind::Categorical | ValueKind::Mask) {
                let grid = &self.data;
                let (nx, ny) = (grid.nx.div_ceil(factor), grid.ny.div_ceil(factor));
                let mut values = Vec::with_capacity(nx * ny);
                for y in 0..ny {
                    for x in 0..nx {
                        let sx = ((x as f64 + 0.5) * grid.nx as f64 / nx as f64) as usize;
                        let sy = ((y as f64 + 0.5) * grid.ny as f64 / ny as f64) as usize;
                        values.push(
                            grid.values
                                .get(sy * grid.nx + sx)
                                .copied()
                                .unwrap_or(f32::NAN),
                        );
                    }
                }
                self.data.values = values;
                self.data.nx = nx;
                self.data.ny = ny;
                metadata.transform = DisplayTransform::NearestCell;
            } else {
                self.data = self.data.decimated(max_dim.max(1));
                metadata.transform = DisplayTransform::MaximumPool { factor };
            }
            metadata.displayed = (&self.data).into();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{DataStamp, QualitySummary};
    use chrono::{DateTime, Duration, Utc};

    fn t(min: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_780_000_000, 0).unwrap() + Duration::minutes(min)
    }

    fn frame(min: i64, values: Vec<f32>) -> Stamped<MrmsField> {
        let data = MrmsField {
            values,
            nx: 2,
            ny: 2,
            lon_west: -98.0,
            lon_east: -97.0,
            lat_north: 36.0,
            lat_south: 35.0,
            time: t(min),
        };
        Stamped {
            stamp: DataStamp {
                source_id: "mrms".into(),
                product_id: "Reflectivity".into(),
                issue_time: None,
                run_time: None,
                valid_time: t(min),
                received_time: t(min) + Duration::seconds(40),
                source_latency: None,
                is_forecast: false,
                is_derived: false,
                quality: QualitySummary::Unknown,
                grid: None,
            },
            data,
        }
    }

    #[test]
    fn a_blend_lies_between_its_frames_and_says_so() {
        let a = frame(0, vec![10.0, 20.0, f32::NAN, 40.0]);
        let b = frame(4, vec![30.0, 20.0, 50.0, f32::NAN]);
        let m = blend_frames(&a, &b, t(1), ValueKind::Scalar).unwrap();
        assert_eq!(m.data.values[..2], [15.0, 20.0]);
        assert!(
            m.data.values[2].is_nan() && m.data.values[3].is_nan(),
            "a cell missing in either frame stays missing"
        );
        assert_eq!((m.data.time, m.stamp.valid_time), (t(1), t(1)));
        assert!(m.stamp.is_derived);
        assert_eq!(m.stamp.received_time, b.stamp.received_time);
        let blend = m.stamp.grid.as_ref().unwrap().blend.unwrap();
        assert_eq!((blend.before, blend.after), (t(0), t(4)));
        assert!((blend.weight_after - 0.25).abs() < 1e-12);
        // Display reduction keeps the label.
        let shown = m.clone().for_display(1, ValueKind::Scalar);
        assert_eq!(shown.stamp.grid.unwrap().blend, Some(blend));
        // It travels in exports, and a provenance written before it reads back without one.
        let json = serde_json::to_string(&m.stamp).unwrap();
        assert!(json.contains("\"blend\""), "{json}");
        let plain = serde_json::to_string(&GridProvenance::native(&a.data)).unwrap();
        assert!(!plain.contains("blend"));
        let back: GridProvenance = serde_json::from_str(&plain).unwrap();
        assert_eq!(back.blend, None);
    }

    #[test]
    fn categories_other_grids_and_other_products_are_not_blended() {
        let a = frame(0, vec![1.0; 4]);
        let b = frame(4, vec![3.0; 4]);
        for kind in [
            ValueKind::Categorical,
            ValueKind::Mask,
            ValueKind::Vector,
            ValueKind::Accumulation,
        ] {
            assert_eq!(
                blend_frames(&a, &b, t(2), kind).err(),
                Some(BlendRefused::Kind(kind))
            );
        }
        assert!(blend_frames(&a, &b, t(2), ValueKind::Probability).is_ok());
        let mut moved = b.clone();
        moved.data.lon_west -= 0.5;
        assert_eq!(
            blend_frames(&a, &moved, t(2), ValueKind::Scalar).err(),
            Some(BlendRefused::Grid)
        );
        let mut other = b.clone();
        other.stamp.product_id = "QPE".into();
        assert_eq!(
            blend_frames(&a, &other, t(2), ValueKind::Scalar).err(),
            Some(BlendRefused::Product)
        );
        for at in [t(0), t(4), t(5)] {
            assert_eq!(
                blend_frames(&a, &b, at, ValueKind::Scalar).err(),
                Some(BlendRefused::Order)
            );
        }
    }
}
