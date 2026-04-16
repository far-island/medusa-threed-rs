// Bowyer-Watson 2D Delaunay triangulation.
//
// Ported from Delaunator.java.
// Operates on XY projection of 3D points — Z is preserved in output
// vertices but does not affect triangulation.

use std::collections::{HashSet, LinkedList};

/// Output triangle mesh: flat vertex array + face index array.
pub struct TriangulationResult {
    /// Flat xyz triples for unique vertices.
    pub vertices: Vec<f32>,
    /// Flat face indices (triples of vertex indices into `vertices`).
    pub faces: Vec<i32>,
    pub triangle_count: i32,
    pub vertex_count: i32,
}

// Internal types for the algorithm.

#[derive(Clone)]
struct Point {
    x: f64,
    y: f64,
    z: f64,
    id: i32,
}

#[derive(Clone)]
struct Triangle {
    p1: Point,
    p2: Point,
    p3: Point,
    e1: Edge,
    e2: Edge,
    e3: Edge,
}

impl Triangle {
    fn new(p1: &Point, p2: &Point, p3: &Point) -> Self {
        Triangle {
            e1: Edge::new(p1, p2),
            e2: Edge::new(p2, p3),
            e3: Edge::new(p1, p3),
            p1: p1.clone(),
            p2: p2.clone(),
            p3: p3.clone(),
        }
    }
}

#[derive(Clone)]
struct Edge {
    p1: Point,
    p2: Point,
}

impl Edge {
    fn new(p1: &Point, p2: &Point) -> Self {
        Edge {
            p1: p1.clone(),
            p2: p2.clone(),
        }
    }

    fn key(&self) -> (i64, i64, i64, i64) {
        // Canonical key: order by id so (a,b) == (b,a)
        let a_x = self.p1.x.to_bits() as i64;
        let a_y = self.p1.y.to_bits() as i64;
        let b_x = self.p2.x.to_bits() as i64;
        let b_y = self.p2.y.to_bits() as i64;
        if (a_x, a_y) <= (b_x, b_y) {
            (a_x, a_y, b_x, b_y)
        } else {
            (b_x, b_y, a_x, a_y)
        }
    }
}

impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}
impl Eq for Edge {}
impl std::hash::Hash for Edge {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

fn ccw(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> bool {
    (bx - ax) * (cy - ay) - (cx - ax) * (by - ay) > 0.0
}

fn in_circle(pt: &Point, v1: &Point, v2: &Point, v3: &Point) -> bool {
    let ax = v1.x - pt.x;
    let ay = v1.y - pt.y;
    let bx = v2.x - pt.x;
    let by = v2.y - pt.y;
    let cx = v3.x - pt.x;
    let cy = v3.y - pt.y;

    let det = (ax * ax + ay * ay) * (bx * cy - cx * by)
        - (bx * bx + by * by) * (ax * cy - cx * ay)
        + (cx * cx + cy * cy) * (ax * by - bx * ay);

    if ccw(v1.x, v1.y, v2.x, v2.y, v3.x, v3.y) {
        det > 0.0
    } else {
        det < 0.0
    }
}

fn max_abs_coordinate(points: &[Point]) -> f64 {
    let mut m: f64 = 0.0;
    for p in points {
        m = m.max(p.x.abs()).max(p.y.abs());
    }
    m
}

fn has_no_super_vertex(tri: &Triangle, super_tri: &Triangle) -> bool {
    let sv = [&super_tri.p1, &super_tri.p2, &super_tri.p3];
    let tv = [&tri.p1, &tri.p2, &tri.p3];
    for s in &sv {
        for t in &tv {
            if (s.x == t.x) && (s.y == t.y) && (s.z == t.z) {
                return false;
            }
        }
    }
    true
}

/// Run Bowyer-Watson Delaunay triangulation on XY projection.
/// `positions` is a flat f32 array of xyz triples.
/// Returns a TriangulationResult with flat vertices and face indices.
pub fn triangulate(positions: &[f32]) -> TriangulationResult {
    let n = positions.len() / 3;
    if n < 3 {
        return TriangulationResult {
            vertices: vec![],
            faces: vec![],
            triangle_count: 0,
            vertex_count: 0,
        };
    }

    // Convert to internal Point type
    let mut points: Vec<Point> = (0..n)
        .map(|i| Point {
            x: positions[i * 3] as f64,
            y: positions[i * 3 + 1] as f64,
            z: positions[i * 3 + 2] as f64,
            id: i as i32 + 1, // 1-based, 0 reserved
        })
        .collect();

    let m = max_abs_coordinate(&points);
    let super_tri = Triangle::new(
        &Point { x: 3.0 * m, y: 0.0, z: 0.0, id: -1 },
        &Point { x: 0.0, y: 3.0 * m, z: 0.0, id: -2 },
        &Point { x: -3.0 * m, y: -3.0 * m, z: 0.0, id: -3 },
    );

    let mut triangulation: LinkedList<Triangle> = LinkedList::new();
    triangulation.push_back(super_tri.clone());
    let mut solution: Vec<Triangle> = Vec::new();

    for point in &mut points {
        let mut edge_first: HashSet<Edge> = HashSet::new();
        let mut polygon: HashSet<Edge> = HashSet::new();

        // Find bad triangles and build polygon
        let mut kept: LinkedList<Triangle> = LinkedList::new();
        for tri in triangulation.iter() {
            if in_circle(point, &tri.p1, &tri.p2, &tri.p3) {
                // Remove from solution
                solution.retain(|s| !triangles_eq(s, tri));

                for edge in [&tri.e1, &tri.e2, &tri.e3] {
                    if edge_first.contains(edge) {
                        polygon.remove(edge);
                    } else {
                        edge_first.insert(edge.clone());
                        polygon.insert(edge.clone());
                    }
                }
            } else {
                kept.push_back(tri.clone());
            }
        }
        triangulation = kept;

        // Create new triangles from polygon edges
        for edge in &polygon {
            let new_tri = Triangle::new(point, &edge.p1, &edge.p2);
            if has_no_super_vertex(&new_tri, &super_tri) {
                solution.push(new_tri.clone());
            }
            triangulation.push_back(new_tri);
        }
    }

    // Convert solution to flat arrays
    build_mesh_from_triangles(&solution)
}

fn triangles_eq(a: &Triangle, b: &Triangle) -> bool {
    points_eq(&a.p1, &b.p1) && points_eq(&a.p2, &b.p2) && points_eq(&a.p3, &b.p3)
}

fn points_eq(a: &Point, b: &Point) -> bool {
    a.x == b.x && a.y == b.y && a.z == b.z
}

fn build_mesh_from_triangles(triangles: &[Triangle]) -> TriangulationResult {
    // Collect unique vertices, assign indices
    let mut vertex_map: std::collections::HashMap<i32, (usize, f64, f64, f64)> =
        std::collections::HashMap::new();
    let mut next_idx: usize = 0;

    let get_idx =
        |p: &Point, map: &mut std::collections::HashMap<i32, (usize, f64, f64, f64)>,
         next: &mut usize| -> usize {
            if let Some(&(idx, _, _, _)) = map.get(&p.id) {
                idx
            } else {
                let idx = *next;
                map.insert(p.id, (idx, p.x, p.y, p.z));
                *next += 1;
                idx
            }
        };

    let mut faces = Vec::with_capacity(triangles.len() * 3);
    for tri in triangles {
        let i1 = get_idx(&tri.p1, &mut vertex_map, &mut next_idx);
        let i2 = get_idx(&tri.p2, &mut vertex_map, &mut next_idx);
        let i3 = get_idx(&tri.p3, &mut vertex_map, &mut next_idx);
        faces.extend_from_slice(&[i1 as i32, i2 as i32, i3 as i32]);
    }

    let vertex_count = vertex_map.len();
    let mut vertices = vec![0.0f32; vertex_count * 3];
    for (_, &(idx, x, y, z)) in &vertex_map {
        vertices[idx * 3] = x as f32;
        vertices[idx * 3 + 1] = y as f32;
        vertices[idx * 3 + 2] = z as f32;
    }

    TriangulationResult {
        vertices,
        faces,
        triangle_count: triangles.len() as i32,
        vertex_count: vertex_count as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_too_few_points() {
        let r = triangulate(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(r.triangle_count, 0);
    }

    #[test]
    fn test_three_points() {
        // Simplest case: 3 non-collinear points -> 1 triangle
        let positions = vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 5.0, 10.0, 0.0];
        let r = triangulate(&positions);
        assert_eq!(r.triangle_count, 1);
        assert_eq!(r.vertex_count, 3);
        assert_eq!(r.faces.len(), 3);
        assert_eq!(r.vertices.len(), 9);
    }

    #[test]
    fn test_four_points_square() {
        // 4 points forming a square -> 2 triangles
        let positions = vec![
            0.0, 0.0, 0.0,
            10.0, 0.0, 0.0,
            10.0, 10.0, 0.0,
            0.0, 10.0, 0.0,
        ];
        let r = triangulate(&positions);
        assert_eq!(r.vertex_count, 4);
        assert_eq!(r.triangle_count, 2);
        assert_eq!(r.faces.len(), 6);
    }

    #[test]
    fn test_preserves_z() {
        let positions = vec![
            0.0, 0.0, 5.0,
            10.0, 0.0, 15.0,
            5.0, 10.0, 25.0,
        ];
        let r = triangulate(&positions);
        // All Z values should be in the output vertices
        let z_values: Vec<f32> = r.vertices.chunks(3).map(|c| c[2]).collect();
        assert!(z_values.contains(&5.0));
        assert!(z_values.contains(&15.0));
        assert!(z_values.contains(&25.0));
    }

    #[test]
    fn test_larger_cloud() {
        // 9 points in a 3x3 grid
        let mut positions = Vec::new();
        for y in 0..3 {
            for x in 0..3 {
                positions.extend_from_slice(&[x as f32 * 10.0, y as f32 * 10.0, 0.0]);
            }
        }
        let r = triangulate(&positions);
        assert_eq!(r.vertex_count, 9);
        // A 3x3 grid should produce at least 8 triangles (2 per quad cell)
        assert!(r.triangle_count >= 8, "got {} triangles", r.triangle_count);
    }
}
