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
///
/// `colors` is left empty: the metrology detector has no per-point color
/// data (HDetector yields a pixel-position profile, not pixel intensities),
/// so the server emits no RGB. Java client lets the viewer apply its
/// z-based colormap — which is the UX legacy produced visually, even if
/// the legacy `new PPoint(x,y,z,222,222,222)` path stored those bytes into
/// `normal_x/y/z` via the overload-resolution quirk (see medusa-3d issue
/// #3). Emitting empty `colors` here converges the gRPC path with what the
/// viewer actually renders and avoids baking the legacy bug into the wire.
pub struct ProfilePoints {
    /// Flat xyz triples.
    pub positions: Vec<f32>,
    /// Empty in the metrology pipeline; reserved for future detectors that
    /// would supply real per-point color. See doc above.
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

    let len = left_upper.len().min(right_lower.len());

    for i in 0..len {
        // Left/upper profile
        if left_upper[i] != 0.0 {
            let z = i as f64 * step * pixel_density_vertical;
            let r = (left_upper[i] - center) * pixel_density_horizontal;
            let x = r * sin_a;
            let y = r * cos_a;

            positions.extend_from_slice(&[x as f32, y as f32, z as f32]);
        }

        // Right/lower profile
        if left_upper[i] != 0.0 && right_lower[i] != 0.0 {
            let z = i as f64 * step * pixel_density_vertical;
            let r = (right_lower[i] - center) * pixel_density_horizontal;
            let x = r * sin_a;
            let y = r * cos_a;

            positions.extend_from_slice(&[x as f32, y as f32, z as f32]);
        }
    }

    ProfilePoints {
        positions,
        colors: Vec::new(),
    }
}

/// List PNG slice files in a single directory.
///
/// The legacy Java pipeline treats "dataset path" as a directory of PNGs
/// named `<angle_radians>.png`. This helper matches that contract exactly
/// without the parent-directory rescan dance used by `list_datasets`.
///
/// Returns slices sorted by filename for deterministic stream ordering.
pub fn list_slices_in_dir(dir: &Path) -> Vec<SliceInfo> {
    let mut slices = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return slices,
    };
    for file in entries.flatten() {
        let fpath = file.path();
        if !fpath.is_file() {
            continue;
        }
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
    slices.sort_by(|a, b| a.filename.cmp(&b.filename));
    slices
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

        // G1: no 222-gray emitted from the detector pipeline (medusa-3d
        // issue #3). Colors stays empty; the client's viewer applies the
        // z-based colormap it already used under legacy.
        assert!(
            result.colors.is_empty(),
            "profile_to_points must emit empty colors (G1)"
        );
    }

    #[test]
    fn test_profile_to_points_skip_zero() {
        let left = vec![0.0, 10.0];
        let right = vec![0.0, 20.0];
        let result = profile_to_points(&left, &right, 1.0, 30.0, 0.0, 1.0, 1.0);

        // Row 0 skipped (left=0), row 1 produces 2 points
        assert_eq!(result.positions.len(), 6);
        assert!(result.colors.is_empty());
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

    #[test]
    fn test_profile_to_points_empty_profile() {
        let result = profile_to_points(&[], &[], 1.0, 30.0, 0.0, 1.0, 1.0);
        assert!(result.positions.is_empty());
        assert!(result.colors.is_empty());
    }

    #[test]
    fn test_profile_to_points_no_detection_row() {
        // All-zero profiles produce no points — matches legacy
        // `if (leftUpperProfile[i] == 0) continue` in ThreeDController.
        let left = vec![0.0, 0.0, 0.0];
        let right = vec![0.0, 0.0, 0.0];
        let result = profile_to_points(&left, &right, 1.0, 30.0, 0.0, 1.0, 1.0);
        assert!(result.positions.is_empty());
    }

    #[test]
    fn test_list_slices_in_dir_direct() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("0.0.png"), b"fake").unwrap();
        fs::write(dir.path().join("1.5708.png"), b"fake").unwrap();
        fs::write(dir.path().join("ignore.txt"), b"not a slice").unwrap();

        let slices = list_slices_in_dir(dir.path());
        assert_eq!(slices.len(), 2);
        assert_eq!(slices[0].filename, "0.0.png");
        assert_eq!(slices[1].filename, "1.5708.png");
        assert!((slices[0].angle - 0.0).abs() < 1e-6);
        assert!((slices[1].angle - 1.5708).abs() < 1e-4);
    }

    #[test]
    fn test_list_slices_in_dir_nonexistent() {
        let slices = list_slices_in_dir(Path::new("/nonexistent/xyz"));
        assert!(slices.is_empty());
    }

    #[test]
    fn test_list_slices_in_dir_skips_subdirs() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("0.0.png"), b"fake").unwrap();
        std::fs::create_dir(dir.path().join("subdir.png")).unwrap();

        let slices = list_slices_in_dir(dir.path());
        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].filename, "0.0.png");
    }
}
