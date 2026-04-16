// Scan logic: profile-to-3D conversion and dataset discovery.
//
// The profile detection (HDetector) stays in Java for now.
// This module does:
//   1. Dataset/slice filesystem discovery (ported from ThreeDController.onPostInit)
//   2. Profile-to-3D point conversion (ported from ThreeDController.scanAll/scanOne)
//      — polar-to-cartesian math based on rotation angle + pixel density

use std::path::Path;

/// A discovered scan dataset.
pub struct DatasetInfo {
    pub name: String,
    pub path: String,
    pub slices: Vec<SliceInfo>,
}

/// A discovered scan slice.
pub struct SliceInfo {
    pub filename: String,
    pub path: String,
    pub angle: f64,
}

/// Result of converting a profile to 3D points.
pub struct ProfilePoints {
    /// Flat xyz triples.
    pub positions: Vec<f32>,
    /// Flat rgb triples (default grey 222,222,222).
    pub colors: Vec<i32>,
}

/// Discover datasets in a root directory.
/// Each subdirectory is a dataset; PNG files within are slices.
/// Slice angle is parsed from filename (e.g. "0.523598.png" -> 0.523598 radians).
///
/// Ported from ThreeDController.onPostInit.
pub fn list_datasets(root_path: &Path) -> Vec<DatasetInfo> {
    let mut datasets = Vec::new();

    let entries = match std::fs::read_dir(root_path) {
        Ok(e) => e,
        Err(_) => return datasets,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let mut slices = Vec::new();
        if let Ok(files) = std::fs::read_dir(&path) {
            for file in files.flatten() {
                let fpath = file.path();
                let fname = fpath
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if fname.ends_with(".png") {
                    let angle = parse_angle_from_filename(&fname);
                    slices.push(SliceInfo {
                        filename: fname,
                        path: fpath.to_string_lossy().to_string(),
                        angle,
                    });
                }
            }
        }

        // Sort slices by filename for deterministic order
        slices.sort_by(|a, b| a.filename.partial_cmp(&b.filename).unwrap_or(std::cmp::Ordering::Equal));

        datasets.push(DatasetInfo {
            name,
            path: path.to_string_lossy().to_string(),
            slices,
        });
    }

    datasets.sort_by(|a, b| a.name.cmp(&b.name));
    datasets
}

/// Parse rotation angle from slice filename.
/// Filename format: "<angle_radians>.png" (e.g. "0.523598.png").
/// Returns 0.0 if parsing fails.
fn parse_angle_from_filename(filename: &str) -> f64 {
    let stem = filename.strip_suffix(".png").unwrap_or(filename);
    stem.parse::<f64>().unwrap_or(0.0)
}

/// Convert a detected profile to 3D points.
///
/// This is the polar-to-cartesian conversion from ThreeDController.scanAll:
///   z = row_index * step * pixel_density_vertical
///   r = (profile_value - center) * pixel_density_horizontal
///   x = r * sin(angle)
///   y = r * cos(angle)
///
/// Both left/upper and right/lower profiles are processed.
pub fn profile_to_points(
    left_upper: &[f64],
    right_lower: &[f64],
    step: f64,
    scan_area_height: f64,
    angle: f64,
    pixel_density_horizontal: f64,
    pixel_density_vertical: f64,
) -> ProfilePoints {
    let center = scan_area_height / 2.0;
    let sin_a = angle.sin();
    let cos_a = angle.cos();

    let mut positions = Vec::new();
    let mut colors = Vec::new();

    let len = left_upper.len().min(right_lower.len());

    for i in 0..len {
        // Left/upper profile
        if left_upper[i] != 0.0 {
            let z = i as f64 * step * pixel_density_vertical;
            let r = (left_upper[i] - center) * pixel_density_horizontal;
            let x = r * sin_a;
            let y = r * cos_a;

            positions.extend_from_slice(&[x as f32, y as f32, z as f32]);
            colors.extend_from_slice(&[222, 222, 222]);
        }

        // Right/lower profile
        if left_upper[i] != 0.0 && right_lower[i] != 0.0 {
            let z = i as f64 * step * pixel_density_vertical;
            let r = (right_lower[i] - center) * pixel_density_horizontal;
            let x = r * sin_a;
            let y = r * cos_a;

            positions.extend_from_slice(&[x as f32, y as f32, z as f32]);
            colors.extend_from_slice(&[222, 222, 222]);
        }
    }

    ProfilePoints { positions, colors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_angle() {
        assert!((parse_angle_from_filename("0.523598.png") - 0.523598).abs() < 1e-6);
        assert!((parse_angle_from_filename("3.14159.png") - 3.14159).abs() < 1e-5);
        assert_eq!(parse_angle_from_filename("invalid.png"), 0.0);
        assert_eq!(parse_angle_from_filename("nopng"), 0.0);
    }

    #[test]
    fn test_list_datasets_empty() {
        let dir = tempfile::tempdir().unwrap();
        let datasets = list_datasets(dir.path());
        assert!(datasets.is_empty());
    }

    #[test]
    fn test_list_datasets_with_slices() {
        let dir = tempfile::tempdir().unwrap();
        let ds_dir = dir.path().join("dataset1");
        fs::create_dir(&ds_dir).unwrap();
        fs::write(ds_dir.join("0.0.png"), b"fake").unwrap();
        fs::write(ds_dir.join("1.5708.png"), b"fake").unwrap();
        fs::write(ds_dir.join("readme.txt"), b"ignore").unwrap();

        let datasets = list_datasets(dir.path());
        assert_eq!(datasets.len(), 1);
        assert_eq!(datasets[0].name, "dataset1");
        assert_eq!(datasets[0].slices.len(), 2);
        assert!((datasets[0].slices[0].angle - 0.0).abs() < 1e-6);
        assert!((datasets[0].slices[1].angle - 1.5708).abs() < 1e-4);
    }

    #[test]
    fn test_list_datasets_nonexistent() {
        let datasets = list_datasets(Path::new("/nonexistent/path"));
        assert!(datasets.is_empty());
    }

    #[test]
    fn test_profile_to_points_basic() {
        // Simple case: one row, angle=0 (sin=0, cos=1), step=1, density=1
        let left = vec![10.0];
        let right = vec![20.0];
        let step = 1.0;
        let scan_height = 30.0; // center = 15
        let angle = 0.0;
        let pdh = 1.0;
        let pdv = 1.0;

        let result = profile_to_points(&left, &right, step, scan_height, angle, pdh, pdv);

        // Left: r = (10 - 15) * 1 = -5, x = -5*sin(0) = 0, y = -5*cos(0) = -5, z = 0
        assert_eq!(result.positions.len(), 6); // 2 points * 3
        assert!((result.positions[0] - 0.0).abs() < 1e-6); // x
        assert!((result.positions[1] - (-5.0)).abs() < 1e-6); // y
        assert!((result.positions[2] - 0.0).abs() < 1e-6); // z

        // Right: r = (20 - 15) * 1 = 5, x = 5*sin(0) = 0, y = 5*cos(0) = 5, z = 0
        assert!((result.positions[3] - 0.0).abs() < 1e-6); // x
        assert!((result.positions[4] - 5.0).abs() < 1e-6); // y
    }

    #[test]
    fn test_profile_to_points_skip_zero() {
        let left = vec![0.0, 10.0];
        let right = vec![0.0, 20.0];
        let result = profile_to_points(&left, &right, 1.0, 30.0, 0.0, 1.0, 1.0);

        // Row 0 skipped (left=0), row 1 produces 2 points
        assert_eq!(result.positions.len(), 6);
    }

    #[test]
    fn test_profile_to_points_with_angle() {
        let left = vec![20.0]; // r = (20 - 15) * 1 = 5
        let right = vec![10.0]; // r = (10 - 15) * 1 = -5
        let angle = std::f64::consts::FRAC_PI_2; // 90 degrees
        let result = profile_to_points(&left, &right, 1.0, 30.0, angle, 1.0, 1.0);

        // Left: x = 5*sin(pi/2) = 5, y = 5*cos(pi/2) ~ 0
        assert!((result.positions[0] - 5.0).abs() < 1e-6);
        assert!(result.positions[1].abs() < 1e-6);
    }
}
