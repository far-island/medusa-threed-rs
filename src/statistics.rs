// Point cloud statistics: bounding box, center of mass, display size.
//
// Ported from ScaleConfiguration.java.

/// Result of computing point cloud statistics.
pub struct CloudStatistics {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
    pub min_z: f64,
    pub max_z: f64,
    pub center_x: f64,
    pub center_y: f64,
    pub center_z: f64,
    pub point_count: i32,
    /// Estimate of minimum inter-point distance (display size hint).
    pub display_size: f64,
    /// Maximum absolute coordinate value.
    pub raw_scale_factor: f64,
}

/// Compute bounding box, center of mass, and display size for a point cloud.
/// `positions` is a flat f32 array of xyz triples.
pub fn compute_statistics(positions: &[f32]) -> CloudStatistics {
    let n = positions.len() / 3;
    if n == 0 {
        return CloudStatistics {
            min_x: 0.0,
            max_x: 0.0,
            min_y: 0.0,
            max_y: 0.0,
            min_z: 0.0,
            max_z: 0.0,
            center_x: 0.0,
            center_y: 0.0,
            center_z: 0.0,
            point_count: 0,
            display_size: 0.0,
            raw_scale_factor: 0.0,
        };
    }

    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    let mut min_z = f64::MAX;
    let mut max_z = f64::MIN;
    let mut sum_x: f64 = 0.0;
    let mut sum_y: f64 = 0.0;
    let mut sum_z: f64 = 0.0;
    let mut raw_scale: f64 = 0.0;

    for i in 0..n {
        let x = positions[i * 3] as f64;
        let y = positions[i * 3 + 1] as f64;
        let z = positions[i * 3 + 2] as f64;

        if x < min_x { min_x = x; }
        if x > max_x { max_x = x; }
        if y < min_y { min_y = y; }
        if y > max_y { max_y = y; }
        if z < min_z { min_z = z; }
        if z > max_z { max_z = z; }

        sum_x += x;
        sum_y += y;
        sum_z += z;

        raw_scale = raw_scale.max(x.abs()).max(y.abs()).max(z.abs());
    }

    let nf = n as f64;
    let center_x = sum_x / nf;
    let center_y = sum_y / nf;
    let center_z = sum_z / nf;

    // Display size: estimate minimum inter-point distance using a
    // divide-and-conquer approach on a sample.  For large clouds we
    // sample to keep it O(n) rather than O(n^2).
    // Matches ScaleConfiguration.calculateMinDis (approximate).
    let display_size = estimate_min_distance(positions, n);

    CloudStatistics {
        min_x,
        max_x,
        min_y,
        max_y,
        min_z,
        max_z,
        center_x,
        center_y,
        center_z,
        point_count: n as i32,
        display_size,
        raw_scale_factor: raw_scale,
    }
}

/// Estimate minimum inter-point distance.
/// Uses a sampled brute-force approach for speed.
fn estimate_min_distance(positions: &[f32], n: usize) -> f64 {
    if n < 2 {
        return 0.0;
    }

    // For large clouds, sample up to 1000 points
    let sample_size = n.min(1000);
    let step = if n > sample_size { n / sample_size } else { 1 };

    let mut min_dist = f64::MAX;

    // Compare each sampled point against its next few neighbours in the array
    let neighbours = 7usize; // matches Java's constant of 6+1
    for si in (0..n).step_by(step) {
        let x1 = positions[si * 3] as f64;
        let y1 = positions[si * 3 + 1] as f64;
        let z1 = positions[si * 3 + 2] as f64;

        let end = (si + neighbours).min(n);
        for sj in (si + 1)..end {
            let x2 = positions[sj * 3] as f64;
            let y2 = positions[sj * 3 + 1] as f64;
            let z2 = positions[sj * 3 + 2] as f64;

            let d = ((x1 - x2).powi(2) + (y1 - y2).powi(2) + (z1 - z2).powi(2)).sqrt();
            if d > 0.0 && d < min_dist {
                min_dist = d;
            }
        }
    }

    if min_dist == f64::MAX { 0.0 } else { min_dist }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_cloud() {
        let s = compute_statistics(&[]);
        assert_eq!(s.point_count, 0);
        assert_eq!(s.display_size, 0.0);
    }

    #[test]
    fn test_single_point() {
        let s = compute_statistics(&[1.0, 2.0, 3.0]);
        assert_eq!(s.point_count, 1);
        assert_eq!(s.min_x, 1.0);
        assert_eq!(s.max_x, 1.0);
        assert_eq!(s.center_x, 1.0);
        assert_eq!(s.center_y, 2.0);
        assert_eq!(s.center_z, 3.0);
        assert_eq!(s.raw_scale_factor, 3.0);
    }

    #[test]
    fn test_two_points() {
        let positions = vec![0.0, 0.0, 0.0, 3.0, 4.0, 0.0];
        let s = compute_statistics(&positions);
        assert_eq!(s.point_count, 2);
        assert_eq!(s.min_x, 0.0);
        assert_eq!(s.max_x, 3.0);
        assert_eq!(s.center_x, 1.5);
        assert_eq!(s.center_y, 2.0);
        assert!((s.display_size - 5.0).abs() < 1e-6); // distance = sqrt(9+16) = 5
    }

    #[test]
    fn test_bounding_box() {
        let positions = vec![
            -10.0, 5.0, 0.0,
            10.0, -5.0, 20.0,
            0.0, 0.0, 10.0,
        ];
        let s = compute_statistics(&positions);
        assert_eq!(s.min_x, -10.0);
        assert_eq!(s.max_x, 10.0);
        assert_eq!(s.min_y, -5.0);
        assert_eq!(s.max_y, 5.0);
        assert_eq!(s.min_z, 0.0);
        assert_eq!(s.max_z, 20.0);
        assert_eq!(s.raw_scale_factor, 20.0);
    }
}
