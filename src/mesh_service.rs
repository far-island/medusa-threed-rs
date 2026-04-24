// MeshService gRPC implementation.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Component, Path};

use tonic::{Request, Response, Status};

use crate::proto::farisland::threed::v1::{
    self as pb,
    mesh_service_server::MeshService,
};
use crate::stl;

pub struct MeshServiceImpl;

#[tonic::async_trait]
impl MeshService for MeshServiceImpl {
    async fn triangulate(
        &self,
        request: Request<pb::TriangulateRequest>,
    ) -> Result<Response<pb::TriangulateResponse>, Status> {
        let req = request.into_inner();
        let points = req.points.ok_or_else(|| Status::invalid_argument("points required"))?;

        let result = crate::delaunator::triangulate(&points.positions);

        Ok(Response::new(pb::TriangulateResponse {
            mesh: Some(pb::MeshData {
                vertices: result.vertices,
                faces: result.faces,
                tex_coords: vec![],
            }),
            triangle_count: result.triangle_count,
            vertex_count: result.vertex_count,
        }))
    }

    async fn grid_mesh(
        &self,
        request: Request<pb::GridMeshRequest>,
    ) -> Result<Response<pb::GridMeshResponse>, Status> {
        let req = request.into_inner();
        let points = req.points.ok_or_else(|| Status::invalid_argument("points required"))?;

        let grid_step = if req.grid_step > 0.0 { req.grid_step } else { 1.0 };
        let max_gap = if req.max_gap > 0 { req.max_gap } else { 2 };

        match crate::grid_mesher::grid_mesh(&points.positions, grid_step, max_gap) {
            Some(result) => Ok(Response::new(pb::GridMeshResponse {
                mesh: Some(pb::MeshData {
                    vertices: result.vertices,
                    faces: result.faces,
                    tex_coords: result.tex_coords,
                }),
                grid_cols: result.grid_cols,
                grid_rows: result.grid_rows,
                triangle_count: result.triangle_count,
                vertex_count: result.vertex_count,
            })),
            None => Err(Status::invalid_argument(
                "Cannot generate mesh: not enough points, grid too large, or no adjacent cells",
            )),
        }
    }

    async fn export_mesh(
        &self,
        request: Request<pb::ExportMeshRequest>,
    ) -> Result<Response<pb::ExportMeshResponse>, Status> {
        let req = request.into_inner();
        let format = resolve_format(req.format);

        // Path hygiene: absolute, no ParentDir components.
        if req.output_path.is_empty() {
            return Ok(Response::new(response_err(
                "output_path is empty",
                format,
            )));
        }
        if !is_safe_path(&req.output_path) {
            return Ok(Response::new(response_err("permission denied", format)));
        }

        // Mesh validity: present, nonempty, and tightly-packed triples on
        // both vertices and faces (catches a malformed GridMesh output).
        let Some(mesh) = req.mesh.as_ref() else {
            return Ok(Response::new(response_err("mesh is empty", format)));
        };
        if mesh.vertices.is_empty()
            || mesh.vertices.len() % 3 != 0
            || mesh.faces.is_empty()
            || mesh.faces.len() % 3 != 0
        {
            return Ok(Response::new(response_err("mesh is empty", format)));
        }

        match format {
            pb::MeshExportFormat::Stl => {
                let file = match File::create(&req.output_path) {
                    Ok(f) => f,
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        return Ok(Response::new(response_err("permission denied", format)));
                    }
                    Err(_) => {
                        return Ok(Response::new(response_err("write failed", format)));
                    }
                };
                let mut w = BufWriter::new(file);
                match stl::write_binary(mesh, &mut w) {
                    Ok(n) => Ok(Response::new(pb::ExportMeshResponse {
                        success: true,
                        error_message: String::new(),
                        bytes_written: n,
                        format_used: format as i32,
                    })),
                    Err(_) => Ok(Response::new(response_err("write failed", format))),
                }
            }
            pb::MeshExportFormat::Obj | pb::MeshExportFormat::Ply => Ok(Response::new(
                response_err("format not implemented", format),
            )),
            pb::MeshExportFormat::Unspecified => {
                // resolve_format collapses Unspecified -> Stl, this branch
                // is unreachable in practice but guards against future enum
                // additions from medusa-protos before encoders ship.
                Ok(Response::new(response_err("format not implemented", format)))
            }
        }
    }
}

fn resolve_format(raw: i32) -> pb::MeshExportFormat {
    match pb::MeshExportFormat::try_from(raw) {
        Ok(pb::MeshExportFormat::Unspecified) | Err(_) => pb::MeshExportFormat::Stl,
        Ok(fmt) => fmt,
    }
}

fn response_err(message: &str, format: pb::MeshExportFormat) -> pb::ExportMeshResponse {
    pb::ExportMeshResponse {
        success: false,
        error_message: message.to_string(),
        bytes_written: 0,
        format_used: format as i32,
    }
}

fn is_safe_path(path: &str) -> bool {
    let p = Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    for c in p.components() {
        if matches!(c, Component::ParentDir) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_rejected() {
        assert!(!is_safe_path("relative/path.stl"));
    }

    #[test]
    fn parent_dir_escape_rejected() {
        assert!(!is_safe_path("/tmp/../etc/passwd"));
    }

    #[test]
    fn plain_absolute_path_accepted() {
        assert!(is_safe_path("/tmp/output.stl"));
    }

    #[test]
    fn resolve_format_collapses_unspecified_to_stl() {
        assert_eq!(resolve_format(0), pb::MeshExportFormat::Stl);
        assert_eq!(resolve_format(1), pb::MeshExportFormat::Stl);
        assert_eq!(resolve_format(2), pb::MeshExportFormat::Obj);
        assert_eq!(resolve_format(3), pb::MeshExportFormat::Ply);
    }
}
