// MeshService gRPC implementation.

use tonic::{Request, Response, Status};

use crate::proto::farisland::threed::v1::{
    self as pb,
    mesh_service_server::MeshService,
};

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
}
