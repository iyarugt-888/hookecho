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
        Self { nx: grid.nx, ny: grid.ny,
            bounds: [grid.lon_west, grid.lat_south, grid.lon_east, grid.lat_north] }
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
}

impl GridProvenance {
    pub fn native(grid: &MrmsField) -> Self {
        Self { native: grid.into(), displayed: grid.into(), transform: DisplayTransform::Native }
    }
}

impl Stamped<MrmsField> {
    /// Reduce a freshly decoded field for display. Categories are sampled, never max-pooled.
    /// Native geometry is retained; this does not retain the native value array for probing.
    pub fn for_display(mut self, max_dim: usize, kind: ValueKind) -> Self {
        let factor = self.data.nx.max(self.data.ny).div_ceil(max_dim.max(1)).max(1);
        let metadata = self.stamp.grid.get_or_insert_with(|| GridProvenance::native(&self.data));
        if factor > 1 {
            if matches!(kind, ValueKind::Categorical | ValueKind::Mask) {
                let grid = &self.data;
                let (nx, ny) = (grid.nx.div_ceil(factor), grid.ny.div_ceil(factor));
                let mut values = Vec::with_capacity(nx * ny);
                for y in 0..ny {
                    for x in 0..nx {
                        let sx = ((x as f64 + 0.5) * grid.nx as f64 / nx as f64) as usize;
                        let sy = ((y as f64 + 0.5) * grid.ny as f64 / ny as f64) as usize;
                        values.push(grid.values.get(sy * grid.nx + sx).copied().unwrap_or(f32::NAN));
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
