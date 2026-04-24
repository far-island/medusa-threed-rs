// Binary STL encoder — contributed by medusa-ai for PR #15/#16 ExportMesh.
//
// Format (little-endian throughout):
//   80 bytes: header (zero-filled, reserved for vendor strings)
//   4 bytes : uint32 triangle count
//   per triangle (50 bytes):
//       12 bytes: normal vec3 (f32 × 3)
//       36 bytes: three vertex vec3 (f32 × 3 × 3)
//       2 bytes : uint16 attribute byte count (0)
//
// Total file size = 84 + 50 * triangle_count. Deterministic.

use std::io::{self, Write};

use crate::proto::farisland::threed::v1::MeshData;

/// Emit the binary STL representation of `mesh` to `w`. Caller must have
/// validated shape (`vertices.len() % 3 == 0`, `faces.len() % 3 == 0`).
pub fn write_binary<W: Write>(mesh: &MeshData, w: &mut W) -> Result<u64, StlError> {
    let header = [0u8; 80];
    w.write_all(&header)?;
    let triangle_count = (mesh.faces.len() / 3) as u32;
    w.write_all(&triangle_count.to_le_bytes())?;
    let vertex_count = mesh.vertices.len() / 3;
    let mut written: u64 = 84;
    for tri in mesh.faces.chunks_exact(3) {
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if i0 >= vertex_count || i1 >= vertex_count || i2 >= vertex_count {
            return Err(StlError::InvalidFaceIndex);
        }
        let v0 = vertex(mesh, i0);
        let v1 = vertex(mesh, i1);
        let v2 = vertex(mesh, i2);
        let normal = face_normal(v0, v1, v2);
        write_vec3(w, normal)?;
        write_vec3(w, v0)?;
        write_vec3(w, v1)?;
        write_vec3(w, v2)?;
        w.write_all(&0u16.to_le_bytes())?;
        written += 50;
    }
    Ok(written)
}

fn vertex(mesh: &MeshData, i: usize) -> [f32; 3] {
    let base = i * 3;
    [
        mesh.vertices[base],
        mesh.vertices[base + 1],
        mesh.vertices[base + 2],
    ]
}

fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let mag = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if mag > 0.0 {
        [n[0] / mag, n[1] / mag, n[2] / mag]
    } else {
        [0.0, 0.0, 0.0]
    }
}

fn write_vec3<W: Write>(w: &mut W, v: [f32; 3]) -> io::Result<()> {
    w.write_all(&v[0].to_le_bytes())?;
    w.write_all(&v[1].to_le_bytes())?;
    w.write_all(&v[2].to_le_bytes())?;
    Ok(())
}

/// Expected binary STL size in bytes given the triangle count.
#[allow(dead_code)]
pub fn expected_size(triangle_count: usize) -> u64 {
    84 + 50 * triangle_count as u64
}

#[derive(Debug)]
pub enum StlError {
    InvalidFaceIndex,
    Io(io::Error),
}

impl From<io::Error> for StlError {
    fn from(e: io::Error) -> Self {
        StlError::Io(e)
    }
}

impl std::fmt::Display for StlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StlError::InvalidFaceIndex => write!(f, "face index references vertex out of range"),
            StlError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for StlError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_triangle() -> MeshData {
        MeshData {
            vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            faces: vec![0, 1, 2],
            tex_coords: vec![],
        }
    }

    #[test]
    fn size_matches_formula() {
        let mut buf = Vec::new();
        let n = write_binary(&one_triangle(), &mut buf).unwrap();
        assert_eq!(buf.len() as u64, n);
        assert_eq!(n, expected_size(1));
    }

    #[test]
    fn header_zero_filled_and_triangle_count_encoded() {
        let mut buf = Vec::new();
        write_binary(&one_triangle(), &mut buf).unwrap();
        assert!(buf[..80].iter().all(|b| *b == 0));
        assert_eq!(&buf[80..84], &1u32.to_le_bytes());
    }

    #[test]
    fn normal_points_positive_z_for_ccw_triangle() {
        let mut buf = Vec::new();
        write_binary(&one_triangle(), &mut buf).unwrap();
        let nz = f32::from_le_bytes(buf[92..96].try_into().unwrap());
        assert!((nz - 1.0).abs() < 1e-6);
    }

    #[test]
    fn empty_mesh_writes_only_header_and_zero_count() {
        let empty = MeshData {
            vertices: vec![],
            faces: vec![],
            tex_coords: vec![],
        };
        let mut buf = Vec::new();
        let n = write_binary(&empty, &mut buf).unwrap();
        assert_eq!(n, 84);
        assert_eq!(buf.len(), 84);
    }

    #[test]
    fn invalid_face_index_surfaces_error() {
        let bad = MeshData {
            vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            faces: vec![0, 1, 5],
            tex_coords: vec![],
        };
        let mut buf = Vec::new();
        assert!(matches!(
            write_binary(&bad, &mut buf),
            Err(StlError::InvalidFaceIndex)
        ));
    }

    #[test]
    fn two_triangle_mesh_encoded_correctly() {
        let mesh = MeshData {
            vertices: vec![
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0,
            ],
            faces: vec![0, 1, 2, 0, 2, 3],
            tex_coords: vec![],
        };
        let mut buf = Vec::new();
        let n = write_binary(&mesh, &mut buf).unwrap();
        assert_eq!(n, expected_size(2));
        assert_eq!(&buf[80..84], &2u32.to_le_bytes());
    }
}
