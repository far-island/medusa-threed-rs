// Voxel grid downsampling.
//
// Ported from VolumeResampler.java.
// Uses rayon for parallel iteration (replaces Java parallelStream).

use rayon::prelude::*;
use std::collections::HashSet;
use std::sync::Mutex;

/// Result of voxel grid resampling.
pub struct ResampleResult {
    /// Flat xyz triples of output points (voxel centres).
    pub positions: Vec<f32>,
    /// Flat rgb triples of output point colours.
    pub colors: Vec<i32>,
    pub input_count: i32,
    pub output_count: i32,
}

/// Downsample a point cloud using a voxel grid.
///
/// Each occupied voxel produces one output point at the voxel centre.
/// Colour is taken from the first point that falls in each voxel.
///
/// `max_grid_dim` caps the grid size per axis to prevent OOM (default 500
/// in legacy Java code).
pub fn resample(
    positions: &[f32],
    colors: &[i32],
    voxel_size: f32,
    max_grid_dim: i32,
) -> Result<ResampleResult, String> {
    let n = positions.len() / 3;
    if n == 0 || voxel_size <= 0.0 {
        return Ok(ResampleResult {
            positions: vec![],
            colors: vec![],
            input_count: 0,
            output_count: 0,
        });
    }

    let max_dim = if max_grid_dim > 0 { max_grid_dim } else { 500 };

    // Compute bounding box to determine grid range
    let mut max_coord: f32 = 0.0;
    for i in 0..n {
        max_coord = max_coord
            .max(positions[i * 3].abs())
            .max(positions[i * 3 + 1].abs())
            .max(positions[i * 3 + 2].abs());
    }

    // Effective grid step: ensure we don't exceed max_grid_dim
    let grid_step = voxel_size.max(max_coord / max_dim as f32);
    let dim = (max_coord / grid_step).ceil() as usize + 1;

    if dim == 0 {
        return Ok(ResampleResult {
            positions: vec![],
            colors: vec![],
            input_count: n as i32,
            output_count: 0,
        });
    }

    let has_colors = colors.len() == positions.len();

    // Parallel voxel occupation check
    let occupied = Mutex::new(HashSet::new());
    let out_points = Mutex::new(Vec::new());

    (0..n).into_par_iter().for_each(|i| {
        let x = positions[i * 3];
        let y = positions[i * 3 + 1];
        let z = positions[i * 3 + 2];

        let ix = (x.abs() / grid_step) as u32;
        let iy = (y.abs() / grid_step) as u32;
        let iz = (z.abs() / grid_step) as u32;

        let key = (ix, iy, iz);

        // Try to insert; if new, emit a point
        let is_new = {
            let mut set = occupied.lock().unwrap();
            set.insert(key)
        };

        if is_new {
            let cx = ix as f32 * grid_step;
            let cy = iy as f32 * grid_step;
            let cz = iz as f32 * grid_step;

            let (r, g, b) = if has_colors {
                (colors[i * 3], colors[i * 3 + 1], colors[i * 3 + 2])
            } else {
                (222, 222, 222)
            };

            let mut pts = out_points.lock().unwrap();
            pts.push((cx, cy, cz, r, g, b));
        }
    });

    let pts = out_points.into_inner().unwrap();
    let output_count = pts.len() as i32;

    let mut out_positions = Vec::with_capacity(pts.len() * 3);
    let mut out_colors = Vec::with_capacity(pts.len() * 3);

    for (x, y, z, r, g, b) in pts {
        out_positions.extend_from_slice(&[x, y, z]);
        out_colors.extend_from_slice(&[r, g, b]);
    }

    Ok(ResampleResult {
        positions: out_positions,
        colors: out_colors,
        input_count: n as i32,
        output_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        let r = resample(&[], &[], 1.0, 500).unwrap();
        assert_eq!(r.input_count, 0);
        assert_eq!(r.output_count, 0);
    }

    #[test]
    fn test_single_point() {
        let r = resample(&[1.0, 2.0, 3.0], &[100, 150, 200], 1.0, 500).unwrap();
        assert_eq!(r.input_count, 1);
        assert_eq!(r.output_count, 1);
        assert_eq!(r.positions.len(), 3);
        assert_eq!(r.colors.len(), 3);
    }

    #[test]
    fn test_duplicate_voxel() {
        // Two points in the same voxel (grid_step=10, both within same cell)
        let positions = vec![1.0, 1.0, 1.0, 1.5, 1.5, 1.5];
        let colors = vec![100, 100, 100, 200, 200, 200];
        let r = resample(&positions, &colors, 10.0, 500).unwrap();
        assert_eq!(r.input_count, 2);
        assert_eq!(r.output_count, 1);
    }

    #[test]
    fn test_different_voxels() {
        // Two points far apart -> two distinct voxels
        let positions = vec![1.0, 1.0, 1.0, 50.0, 50.0, 50.0];
        let colors = vec![100, 100, 100, 200, 200, 200];
        let r = resample(&positions, &colors, 1.0, 500).unwrap();
        assert_eq!(r.input_count, 2);
        assert_eq!(r.output_count, 2);
    }

    #[test]
    fn test_max_grid_dim_safety() {
        // Ensure max_grid_dim doesn't cause panic
        let positions = vec![1000.0, 1000.0, 1000.0];
        let colors = vec![100, 100, 100];
        let r = resample(&positions, &colors, 0.001, 10).unwrap();
        assert_eq!(r.input_count, 1);
        assert_eq!(r.output_count, 1);
    }
}
