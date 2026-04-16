// Grid-based surface mesher for 2.5D point clouds (laser line scanner).
//
// Ported from GridMesher.java.
// Projects points onto a regular XY grid, averages Z per cell,
// then triangulates adjacent occupied cells.

/// Output of grid meshing.
pub struct GridMeshResult {
    /// Flat xyz triples for mesh vertices.
    pub vertices: Vec<f32>,
    /// Flat face indices (triples of vertex indices).
    pub faces: Vec<i32>,
    /// Texture coordinates: u,v pairs, one per vertex.
    pub tex_coords: Vec<f32>,
    pub grid_cols: i32,
    pub grid_rows: i32,
    pub triangle_count: i32,
    pub vertex_count: i32,
}

/// Create a triangle mesh from a point cloud using grid binning.
///
/// `positions`: flat xyz triples.
/// `grid_step`: cell size in point coordinate units.
/// `max_gap`: maximum cells to bridge (e.g. 2 = skip 1 empty cell).
pub fn grid_mesh(positions: &[f32], grid_step: f64, max_gap: i32) -> Option<GridMeshResult> {
    let n = positions.len() / 3;
    if n < 3 || grid_step <= 0.0 {
        return None;
    }

    // 1. Bounding box
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;

    for i in 0..n {
        let x = positions[i * 3] as f64;
        let y = positions[i * 3 + 1] as f64;
        if x < min_x { min_x = x; }
        if x > max_x { max_x = x; }
        if y < min_y { min_y = y; }
        if y > max_y { max_y = y; }
    }

    // 2. Grid dimensions
    let cols = ((max_x - min_x) / grid_step).ceil() as usize + 1;
    let rows = ((max_y - min_y) / grid_step).ceil() as usize + 1;

    if cols < 2 || rows < 2 {
        return None;
    }
    if cols as u64 * rows as u64 > 10_000_000 {
        return None; // safety limit, matches Java
    }

    // 3. Bin points into grid
    let grid_size = cols * rows;
    let mut z_sum = vec![0.0f64; grid_size];
    let mut z_count = vec![0u32; grid_size];

    for i in 0..n {
        let x = positions[i * 3] as f64;
        let y = positions[i * 3 + 1] as f64;
        let z = positions[i * 3 + 2] as f64;

        let col = ((x - min_x) / grid_step) as usize;
        let row = ((y - min_y) / grid_step) as usize;
        let col = col.min(cols - 1);
        let row = row.min(rows - 1);
        let idx = row * cols + col;
        z_sum[idx] += z;
        z_count[idx] += 1;
    }

    // 4. Build vertex array and index map
    let mut vertex_index = vec![-1i32; grid_size];
    let mut vertex_count: usize = 0;

    for i in 0..grid_size {
        if z_count[i] > 0 {
            vertex_index[i] = vertex_count as i32;
            vertex_count += 1;
        }
    }

    if vertex_count < 3 {
        return None;
    }

    // 5. Create vertices and tex coords
    let mut vertices = vec![0.0f32; vertex_count * 3];
    let mut tex_coords = vec![0.0f32; vertex_count * 2];

    for row in 0..rows {
        for col in 0..cols {
            let gi = row * cols + col;
            if z_count[gi] > 0 {
                let vi = vertex_index[gi] as usize;
                let x = (min_x + col as f64 * grid_step) as f32;
                let y = (min_y + row as f64 * grid_step) as f32;
                let z = (z_sum[gi] / z_count[gi] as f64) as f32;
                vertices[vi * 3] = x;
                vertices[vi * 3 + 1] = y;
                vertices[vi * 3 + 2] = z;
                tex_coords[vi * 2] = col as f32 / (cols - 1) as f32;
                tex_coords[vi * 2 + 1] = row as f32 / (rows - 1) as f32;
            }
        }
    }

    // 6. Triangulate adjacent occupied cells
    let mut faces: Vec<i32> = Vec::new();

    for row in 0..(rows - 1) {
        for col in 0..(cols - 1) {
            let tl = vertex_index[row * cols + col];
            let tr = vertex_index[row * cols + col + 1];
            let bl = vertex_index[(row + 1) * cols + col];
            let br = vertex_index[(row + 1) * cols + col + 1];

            if tl >= 0 && tr >= 0 && bl >= 0 && br >= 0 {
                // Check Z variance — don't bridge huge gaps
                let z_tl = vertices[tl as usize * 3 + 2];
                let z_tr = vertices[tr as usize * 3 + 2];
                let z_bl = vertices[bl as usize * 3 + 2];
                let z_br = vertices[br as usize * 3 + 2];
                let z_max = z_tl.max(z_tr).max(z_bl).max(z_br);
                let z_min = z_tl.min(z_tr).min(z_bl).min(z_br);
                let z_range = z_max - z_min;

                if z_range < (grid_step * max_gap as f64 * 5.0) as f32 {
                    faces.extend_from_slice(&[tl, bl, tr]);
                    faces.extend_from_slice(&[tr, bl, br]);
                }
            } else if max_gap > 1 {
                // Partial triangles with 3 out of 4 vertices
                if tl >= 0 && tr >= 0 && bl >= 0 {
                    faces.extend_from_slice(&[tl, bl, tr]);
                }
                if tl >= 0 && tr >= 0 && br >= 0 {
                    faces.extend_from_slice(&[tl, br, tr]);
                }
                if tl >= 0 && bl >= 0 && br >= 0 {
                    faces.extend_from_slice(&[tl, bl, br]);
                }
                if tr >= 0 && bl >= 0 && br >= 0 {
                    faces.extend_from_slice(&[tr, bl, br]);
                }
            }
        }
    }

    if faces.is_empty() {
        return None;
    }

    let triangle_count = faces.len() as i32 / 3;

    Some(GridMeshResult {
        vertices,
        faces,
        tex_coords,
        grid_cols: cols as i32,
        grid_rows: rows as i32,
        triangle_count,
        vertex_count: vertex_count as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_too_few_points() {
        assert!(grid_mesh(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 1.0, 2).is_none());
    }

    #[test]
    fn test_2x2_grid() {
        // 4 points at corners of a 10x10 square -> 2x2 grid -> 2 triangles
        let positions = vec![
            0.0, 0.0, 1.0,
            10.0, 0.0, 2.0,
            0.0, 10.0, 3.0,
            10.0, 10.0, 4.0,
        ];
        let r = grid_mesh(&positions, 10.0, 2).unwrap();
        assert_eq!(r.vertex_count, 4);
        assert_eq!(r.triangle_count, 2);
        assert_eq!(r.faces.len(), 6);
        assert_eq!(r.grid_cols, 2);
        assert_eq!(r.grid_rows, 2);
    }

    #[test]
    fn test_3x3_grid() {
        // 9 points in a 3x3 pattern
        let mut positions = Vec::new();
        for row in 0..3 {
            for col in 0..3 {
                positions.extend_from_slice(&[
                    col as f32 * 5.0,
                    row as f32 * 5.0,
                    (col + row) as f32,
                ]);
            }
        }
        let r = grid_mesh(&positions, 5.0, 2).unwrap();
        assert_eq!(r.vertex_count, 9);
        // 2x2 quads in a 3x3 grid = 4 quads = 8 triangles
        assert_eq!(r.triangle_count, 8);
    }

    #[test]
    fn test_z_averaging() {
        // Multiple points in the same cell -> Z should be averaged
        let positions = vec![
            0.0, 0.0, 10.0,
            0.5, 0.5, 20.0, // same cell as above at grid_step=5
            5.0, 0.0, 5.0,
            0.0, 5.0, 5.0,
        ];
        let r = grid_mesh(&positions, 5.0, 2).unwrap();
        // The cell at (0,0) should have averaged Z = 15.0
        // Find the vertex at the origin cell
        let mut found_avg = false;
        for i in 0..r.vertex_count as usize {
            let x = r.vertices[i * 3];
            let y = r.vertices[i * 3 + 1];
            let z = r.vertices[i * 3 + 2];
            if x == 0.0 && y == 0.0 {
                assert!((z - 15.0).abs() < 0.01, "Expected Z=15.0, got {z}");
                found_avg = true;
            }
        }
        assert!(found_avg, "Origin vertex not found");
    }

    #[test]
    fn test_tex_coords_range() {
        let positions = vec![
            0.0, 0.0, 0.0,
            10.0, 0.0, 0.0,
            0.0, 10.0, 0.0,
            10.0, 10.0, 0.0,
        ];
        let r = grid_mesh(&positions, 10.0, 2).unwrap();
        // Tex coords should be in [0, 1]
        for &t in &r.tex_coords {
            assert!(t >= 0.0 && t <= 1.0, "tex coord {t} out of range");
        }
    }

    #[test]
    fn test_safety_limit() {
        // Very small grid step on wide bounding box -> should return None (>10M cells)
        let positions = vec![
            0.0, 0.0, 0.0,
            10000.0, 0.0, 0.0,
            0.0, 10000.0, 0.0,
        ];
        assert!(grid_mesh(&positions, 0.001, 2).is_none());
    }
}
