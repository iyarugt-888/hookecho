//! Isosurfaces of a resampled radar volume (ROADMAP_NEW H3): the surface where a moment crosses a
//! threshold — a 50 dBZ core's skin, a ZDR column's outline, a low-CC debris pocket.
//!
//! Naive surface nets rather than marching cubes: every grid cell the surface passes through gets
//! one vertex, at the average of the points where the surface crosses that cell's edges (each
//! found by linear interpolation along the edge), and every grid edge the surface crosses becomes
//! a quad joining the four cells around it. No 256-case table, a watertight mesh, and vertices
//! that already sit smoothly on the surface. The source values are never smoothed: the only
//! interpolation is along a single grid edge, to find where the threshold falls.
//!
//! Coordinates come back in the volume's own radar-relative kilometres (x east, y north, z up),
//! so the caller places the mesh the way it places the volume.

use crate::volume3d::Volume3d;

/// A triangle mesh in radar-relative km.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IsoMesh {
    pub verts: Vec<[f32; 3]>,
    pub tris: Vec<[u32; 3]>,
    /// True when `max_tris` stopped the mesh early (a threshold low enough to wrap every echo).
    pub truncated: bool,
}

/// The surface where `v3` crosses `value` (in the volume's physical units), or `high_inside =
/// false` to treat values *below* the threshold as inside (low CC). At most `max_tris` triangles.
pub fn isosurface(v3: &Volume3d, value: f32, high_inside: bool, max_tris: usize) -> IsoMesh {
    let (n, nz) = (v3.n, v3.nz);
    if n < 2 || nz < 2 || v3.data.len() < n * n * nz {
        return IsoMesh::default();
    }
    let span = (v3.value_max - v3.value_min).max(f32::EPSILON);
    let iso = 2.0 + ((value - v3.value_min) / span).clamp(0.0, 1.0) * 253.0;
    // Signed field: positive inside. Empty voxels read as far outside either way.
    let field = |i: usize, j: usize, k: usize| -> f32 {
        let b = v3.data[i + n * j + n * n * k];
        if b < 2 {
            return -1.0e3;
        }
        if high_inside {
            b as f32 - iso
        } else {
            iso - b as f32
        }
    };
    let km = |i: f32, j: f32, k: f32| -> [f32; 3] {
        let h = v3.half_km;
        [
            -h + 2.0 * h * i / (n - 1) as f32,
            -h + 2.0 * h * j / (n - 1) as f32,
            v3.top_km * k / (nz - 1) as f32,
        ]
    };
    let (cx, cz) = (n - 1, nz - 1);
    let cell = |i: usize, j: usize, k: usize| i + cx * j + cx * cx * k;
    let mut vert_of = vec![u32::MAX; cx * cx * cz];
    let mut mesh = IsoMesh::default();
    const CORNERS: [[usize; 3]; 8] = [
        [0, 0, 0],
        [1, 0, 0],
        [0, 1, 0],
        [1, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [0, 1, 1],
        [1, 1, 1],
    ];
    const EDGES: [[usize; 2]; 12] = [
        [0, 1],
        [2, 3],
        [4, 5],
        [6, 7],
        [0, 2],
        [1, 3],
        [4, 6],
        [5, 7],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];
    // One vertex per cell the surface passes through.
    for k in 0..cz {
        for j in 0..cx {
            for i in 0..cx {
                let f: [f32; 8] = std::array::from_fn(|c| {
                    field(i + CORNERS[c][0], j + CORNERS[c][1], k + CORNERS[c][2])
                });
                let inside = f.iter().filter(|&&x| x > 0.0).count();
                if inside == 0 || inside == 8 {
                    continue;
                }
                let (mut sum, mut count) = ([0.0f32; 3], 0.0f32);
                for [a, b] in EDGES {
                    if (f[a] > 0.0) != (f[b] > 0.0) {
                        let t = (f[a] / (f[a] - f[b])).clamp(0.0, 1.0);
                        for d in 0..3 {
                            let (pa, pb) = (CORNERS[a][d] as f32, CORNERS[b][d] as f32);
                            sum[d] += pa + t * (pb - pa);
                        }
                        count += 1.0;
                    }
                }
                let p = km(
                    i as f32 + sum[0] / count,
                    j as f32 + sum[1] / count,
                    k as f32 + sum[2] / count,
                );
                vert_of[cell(i, j, k)] = mesh.verts.len() as u32;
                mesh.verts.push(p);
            }
        }
    }
    // One quad per crossed grid edge, joining the four cells that share it. The corner order
    // gives a normal along the edge's axis (+x, +y or +z); it is flipped when the edge's lower
    // end is the outside one, so every face points from inside to outside.
    let emit = |q: [usize; 4], flip: bool, mesh: &mut IsoMesh| -> bool {
        let [a, b, c, d] = q.map(|x| vert_of[x]);
        if [a, b, c, d].contains(&u32::MAX) {
            return true;
        }
        if mesh.tris.len() + 2 > max_tris {
            mesh.truncated = true;
            return false;
        }
        if flip {
            mesh.tris.extend([[a, c, b], [a, d, c]]);
        } else {
            mesh.tris.extend([[a, b, c], [a, c, d]]);
        }
        true
    };
    for k in 0..nz {
        for j in 0..n {
            for i in 0..n {
                let here = field(i, j, k) > 0.0;
                // Along x, to (i+1, j, k): cells (i, j-1..j, k-1..k), normal y × z = +x.
                if i < cx
                    && (1..cx).contains(&j)
                    && (1..cz).contains(&k)
                    && here != (field(i + 1, j, k) > 0.0)
                    && !emit(
                        [
                            cell(i, j - 1, k - 1),
                            cell(i, j, k - 1),
                            cell(i, j, k),
                            cell(i, j - 1, k),
                        ],
                        !here,
                        &mut mesh,
                    )
                {
                    return mesh;
                }
                // Along y, to (i, j+1, k): cells (i-1..i, j, k-1..k), normal z × x = +y.
                if j < cx
                    && (1..cx).contains(&i)
                    && (1..cz).contains(&k)
                    && here != (field(i, j + 1, k) > 0.0)
                    && !emit(
                        [
                            cell(i - 1, j, k - 1),
                            cell(i - 1, j, k),
                            cell(i, j, k),
                            cell(i, j, k - 1),
                        ],
                        !here,
                        &mut mesh,
                    )
                {
                    return mesh;
                }
                // Along z, to (i, j, k+1): cells (i-1..i, j-1..j, k), normal x × y = +z.
                if k < cz
                    && (1..cx).contains(&i)
                    && (1..cx).contains(&j)
                    && here != (field(i, j, k + 1) > 0.0)
                    && !emit(
                        [
                            cell(i - 1, j - 1, k),
                            cell(i, j - 1, k),
                            cell(i, j, k),
                            cell(i - 1, j, k),
                        ],
                        !here,
                        &mut mesh,
                    )
                {
                    return mesh;
                }
            }
        }
    }
    mesh
}

/// Explicit display smoothing (H3's "smoothing toggle"): each vertex moves toward the average of
/// its neighbours, `iterations` times, at half strength. It changes the drawn surface only, never
/// the radar values it came from, and the caller offers it as a visible toggle, never by default.
pub fn smooth(mesh: &mut IsoMesh, iterations: usize) {
    let n = mesh.verts.len();
    let mut neighbours: Vec<Vec<u32>> = vec![Vec::new(); n];
    for t in &mesh.tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if !neighbours[a as usize].contains(&b) {
                neighbours[a as usize].push(b);
            }
            if !neighbours[b as usize].contains(&a) {
                neighbours[b as usize].push(a);
            }
        }
    }
    for _ in 0..iterations {
        let prev = mesh.verts.clone();
        for (i, nb) in neighbours.iter().enumerate() {
            if nb.is_empty() {
                continue;
            }
            let mut avg = [0.0f32; 3];
            for &j in nb {
                for d in 0..3 {
                    avg[d] += prev[j as usize][d];
                }
            }
            for d in 0..3 {
                avg[d] /= nb.len() as f32;
                mesh.verts[i][d] = 0.5 * prev[i][d] + 0.5 * avg[d];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ball of high values in an otherwise low volume.
    fn ball(n: usize, radius: f32) -> Volume3d {
        let mut data = vec![2u8; n * n * n];
        let c = (n - 1) as f32 / 2.0;
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let d =
                        ((i as f32 - c).powi(2) + (j as f32 - c).powi(2) + (k as f32 - c).powi(2))
                            .sqrt();
                    data[i + n * j + n * n * k] = if d < radius { 250 } else { 10 };
                }
            }
        }
        Volume3d {
            data,
            n,
            nz: n,
            half_km: 10.0,
            top_km: 20.0,
            value_min: 0.0,
            value_max: 100.0,
        }
    }

    #[test]
    fn a_ball_gives_a_closed_outward_surface_at_its_radius() {
        let v = ball(21, 6.0);
        let m = isosurface(&v, 50.0, true, 100_000);
        assert!(!m.truncated && m.tris.len() > 100, "{} tris", m.tris.len());
        // Every vertex sits near the ball's radius (grid units: 1 cell = 1 km horizontally).
        let centre = [0.0, 0.0, 10.0];
        for p in &m.verts {
            let d = ((p[0] - centre[0]).powi(2)
                + (p[1] - centre[1]).powi(2)
                + (p[2] - centre[2]).powi(2))
            .sqrt();
            // A stepped ball's staircase puts some vertices up to a cell inside the radius.
            assert!((4.0..7.0).contains(&d), "{p:?} at {d}");
        }
        // Closed: every edge is shared by exactly two triangles.
        let mut edges = std::collections::HashMap::new();
        for t in &m.tris {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|&c| c == 2), "open edges");
        // Outward: the signed volume of the mesh is positive.
        let vol: f32 = m
            .tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| m.verts[i as usize]);
                let cross = [
                    b[1] * c[2] - b[2] * c[1],
                    b[2] * c[0] - b[0] * c[2],
                    b[0] * c[1] - b[1] * c[0],
                ];
                (a[0] * cross[0] + a[1] * cross[1] + a[2] * cross[2]) / 6.0
            })
            .sum();
        assert!(vol > 0.0, "{vol}");
        // Low-inside reverses the surface.
        let inv = isosurface(&v, 50.0, false, 100_000);
        assert_eq!(inv.verts.len(), m.verts.len());
    }

    #[test]
    fn smoothing_evens_out_the_staircase_and_keeps_the_mesh() {
        let v = ball(21, 6.0);
        let raw = isosurface(&v, 50.0, true, 100_000);
        let spread = |m: &IsoMesh| {
            let d: Vec<f32> = m
                .verts
                .iter()
                .map(|p| (p[0].powi(2) + p[1].powi(2) + (p[2] - 10.0).powi(2)).sqrt())
                .collect();
            let mean = d.iter().sum::<f32>() / d.len() as f32;
            d.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / d.len() as f32
        };
        let mut smoothed = raw.clone();
        smooth(&mut smoothed, 3);
        assert_eq!(smoothed.tris, raw.tris);
        assert!(spread(&smoothed) < spread(&raw), "a rounder ball");
    }

    #[test]
    fn empty_volume_and_the_triangle_cap() {
        let mut v = ball(11, 3.0);
        assert!(
            isosurface(&v, 99.0, true, 1000).tris.is_empty(),
            "above every value"
        );
        let capped = isosurface(&v, 50.0, true, 10);
        assert!(capped.truncated && capped.tris.len() <= 10);
        v.data.iter_mut().for_each(|b| *b = 0);
        assert!(isosurface(&v, 50.0, true, 1000).tris.is_empty());
    }
}
