// PCD file parser (binary + ASCII) and PTX exporter.
//
// Ported from:
//   - DataReader.java (openBinary, openAscii)
//   - PointCloud.saveToFile() (PTX export)

use std::fs;
use std::io::Write;
use std::path::Path;

/// Parsed point cloud from a PCD file.
pub struct ParsedCloud {
    /// Flat xyz triples.
    pub positions: Vec<f32>,
    /// Flat rgb triples (0-255). Empty if no color data.
    pub colors: Vec<i32>,
    /// Maximum Z value found.
    pub max_z: f32,
    /// true = binary, false = ASCII.
    pub is_binary: bool,
}

/// Binary PCD magic: version int 1234 at bytes 0..4 (big-endian).
const BINARY_MAGIC: i32 = 1234;

fn bytes_to_i32_be(b: &[u8]) -> i32 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn bytes_to_f32_be(b: &[u8]) -> f32 {
    f32::from_bits(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Load a PCD file, auto-detecting binary vs ASCII format.
pub fn load_pcd(path: &Path) -> Result<ParsedCloud, String> {
    let data = fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;

    // Detect binary: first 4 bytes == 1234 (big-endian int)
    if data.len() >= 36 {
        let version = bytes_to_i32_be(&data[0..4]);
        if version == BINARY_MAGIC {
            return load_binary(&data);
        }
    }

    // Fallback: ASCII
    let text = String::from_utf8_lossy(&data);
    load_ascii(&text)
}

fn load_binary(data: &[u8]) -> Result<ParsedCloud, String> {
    let bs: usize = 4; // byte size per field
    let num_points = bytes_to_i32_be(&data[bs..2 * bs]) as usize;

    let header_size: usize = 36; // 9 * 4 bytes (version, numPoints, viewpoint x7)
    let mut positions = Vec::with_capacity(num_points * 3);
    let mut colors = Vec::with_capacity(num_points * 3);
    let mut max_z: f32 = 0.0;

    for i in 0..num_points {
        let offset = header_size + 3 * bs * i;
        if offset + 3 * bs > data.len() {
            break;
        }
        let x = bytes_to_f32_be(&data[offset..offset + bs]);
        let y = bytes_to_f32_be(&data[offset + bs..offset + 2 * bs]);
        let z = bytes_to_f32_be(&data[offset + 2 * bs..offset + 3 * bs]);

        if z > max_z {
            max_z = z;
        }

        positions.extend_from_slice(&[x, y, z]);
        colors.extend_from_slice(&[222, 222, 222]); // default grey, matches Java
    }

    Ok(ParsedCloud {
        positions,
        colors,
        max_z,
        is_binary: true,
    })
}

fn load_ascii(text: &str) -> Result<ParsedCloud, String> {
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut max_z: f32 = 0.0;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let x: f32 = parts[0].parse().map_err(|e| format!("parse x: {e}"))?;
        let y: f32 = parts[1].parse().map_err(|e| format!("parse y: {e}"))?;
        let z: f32 = parts[2].parse().map_err(|e| format!("parse z: {e}"))?;

        // RGB from columns 4,5,6 (column 3 is intensity=1, skipped)
        let (r, g, b) = if parts.len() >= 7 {
            (
                parts[4].parse::<i32>().unwrap_or(222),
                parts[5].parse::<i32>().unwrap_or(222),
                parts[6].parse::<i32>().unwrap_or(222),
            )
        } else {
            (222, 222, 222)
        };

        if z > max_z {
            max_z = z;
        }

        positions.extend_from_slice(&[x, y, z]);
        colors.extend_from_slice(&[r, g, b]);
    }

    Ok(ParsedCloud {
        positions,
        colors,
        max_z,
        is_binary: false,
    })
}

/// Export points to PTX format (ASCII: "x y z 1 r g b" per line).
/// Matches PointCloud.saveToFile() in Java.
pub fn export_ptx(
    path: &Path,
    positions: &[f32],
    colors: &[i32],
) -> Result<usize, String> {
    let num_points = positions.len() / 3;
    let has_colors = colors.len() == positions.len();

    let mut file =
        fs::File::create(path).map_err(|e| format!("Cannot create {}: {e}", path.display()))?;

    for i in 0..num_points {
        let x = positions[i * 3];
        let y = positions[i * 3 + 1];
        let z = positions[i * 3 + 2];
        let (r, g, b) = if has_colors {
            (colors[i * 3], colors[i * 3 + 1], colors[i * 3 + 2])
        } else {
            (0, 0, 0)
        };
        writeln!(file, "{x} {y} {z} 1 {r} {g} {b}")
            .map_err(|e| format!("Write error: {e}"))?;
    }

    Ok(num_points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_ascii() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.pcd");
        {
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "1.0 2.0 3.0 1 100 150 200").unwrap();
            writeln!(f, "4.0 5.0 6.0 1 50 60 70").unwrap();
            writeln!(f, "").unwrap(); // blank line
        }
        let cloud = load_pcd(&path).unwrap();
        assert!(!cloud.is_binary);
        assert_eq!(cloud.positions.len(), 6);
        assert_eq!(cloud.positions[0], 1.0);
        assert_eq!(cloud.positions[5], 6.0);
        assert_eq!(cloud.max_z, 6.0);
        assert_eq!(cloud.colors.len(), 6);
        assert_eq!(cloud.colors[0], 100);
        assert_eq!(cloud.colors[3], 50);
    }

    #[test]
    fn test_load_ascii_no_color() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.pcd");
        {
            let mut f = fs::File::create(&path).unwrap();
            writeln!(f, "1.5 2.5 3.5").unwrap();
        }
        let cloud = load_pcd(&path).unwrap();
        assert_eq!(cloud.positions.len(), 3);
        assert_eq!(cloud.colors, vec![222, 222, 222]);
    }

    #[test]
    fn test_load_binary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.pcd");

        let mut data = Vec::new();
        // Header: version=1234, numPoints=1, viewpoint (7 floats)
        data.extend_from_slice(&1234i32.to_be_bytes());
        data.extend_from_slice(&1i32.to_be_bytes());
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        // Body: one point (1.0, 2.0, 3.0)
        for v in [1.0f32, 2.0, 3.0] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        fs::write(&path, &data).unwrap();

        let cloud = load_pcd(&path).unwrap();
        assert!(cloud.is_binary);
        assert_eq!(cloud.positions.len(), 3);
        assert_eq!(cloud.positions[0], 1.0);
        assert_eq!(cloud.positions[1], 2.0);
        assert_eq!(cloud.positions[2], 3.0);
        assert_eq!(cloud.max_z, 3.0);
    }

    #[test]
    fn test_export_ptx() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.ptx");
        let positions = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        let colors = vec![100, 150, 200, 50, 60, 70];
        let count = export_ptx(&path, &positions, &colors).unwrap();
        assert_eq!(count, 2);

        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("1 2 3 1 100 150 200"));
        assert!(lines[1].contains("4 5 6 1 50 60 70"));
    }
}
