// PointCloudService gRPC implementation.

use std::path::Path;
use tonic::{Request, Response, Status};

use crate::proto::farisland::threed::v1::{
    self as pb,
    point_cloud_service_server::PointCloudService,
};

pub struct PointCloudServiceImpl;

#[tonic::async_trait]
impl PointCloudService for PointCloudServiceImpl {
    async fn load_pcd(
        &self,
        request: Request<pb::LoadPcdRequest>,
    ) -> Result<Response<pb::LoadPcdResponse>, Status> {
        let req = request.into_inner();
        let path = Path::new(&req.file_path);

        let cloud = crate::pcd::load_pcd(path)
            .map_err(|e| Status::internal(format!("LoadPCD failed: {e}")))?;

        let format = if cloud.is_binary {
            pb::PcdFormat::Binary.into()
        } else {
            pb::PcdFormat::Ascii.into()
        };

        Ok(Response::new(pb::LoadPcdResponse {
            points: Some(pb::PointCloudChunk {
                positions: cloud.positions,
                colors: cloud.colors,
                normals: vec![],
            }),
            max_z: cloud.max_z,
            format,
        }))
    }

    async fn export_ptx(
        &self,
        request: Request<pb::ExportPtxRequest>,
    ) -> Result<Response<pb::ExportPtxResponse>, Status> {
        let req = request.into_inner();
        let path = Path::new(&req.output_path);

        let points = req.points.ok_or_else(|| Status::invalid_argument("points required"))?;

        match crate::pcd::export_ptx(path, &points.positions, &points.colors) {
            Ok(count) => Ok(Response::new(pb::ExportPtxResponse {
                success: true,
                error_message: String::new(),
                points_written: count as i32,
            })),
            Err(e) => Ok(Response::new(pb::ExportPtxResponse {
                success: false,
                error_message: e,
                points_written: 0,
            })),
        }
    }

    async fn get_statistics(
        &self,
        request: Request<pb::GetStatisticsRequest>,
    ) -> Result<Response<pb::GetStatisticsResponse>, Status> {
        let req = request.into_inner();
        let points = req.points.ok_or_else(|| Status::invalid_argument("points required"))?;

        let stats = crate::statistics::compute_statistics(&points.positions);

        Ok(Response::new(pb::GetStatisticsResponse {
            statistics: Some(pb::PointCloudStatistics {
                min_x: stats.min_x,
                max_x: stats.max_x,
                min_y: stats.min_y,
                max_y: stats.max_y,
                min_z: stats.min_z,
                max_z: stats.max_z,
                center_x: stats.center_x,
                center_y: stats.center_y,
                center_z: stats.center_z,
                point_count: stats.point_count,
            }),
            display_size: stats.display_size,
            raw_scale_factor: stats.raw_scale_factor,
        }))
    }

    async fn resample(
        &self,
        request: Request<pb::ResampleRequest>,
    ) -> Result<Response<pb::ResampleResponse>, Status> {
        let req = request.into_inner();
        let points = req.points.ok_or_else(|| Status::invalid_argument("points required"))?;

        let result = crate::resample::resample(
            &points.positions,
            &points.colors,
            req.voxel_size,
            req.max_grid_dim,
        )
        .map_err(|e| Status::internal(format!("Resample failed: {e}")))?;

        Ok(Response::new(pb::ResampleResponse {
            points: Some(pb::PointCloudChunk {
                positions: result.positions,
                colors: result.colors,
                normals: vec![],
            }),
            input_count: result.input_count,
            output_count: result.output_count,
        }))
    }

    async fn apply_colormap(
        &self,
        request: Request<pb::ApplyColormapRequest>,
    ) -> Result<Response<pb::ApplyColormapResponse>, Status> {
        let req = request.into_inner();
        let points = req.points.ok_or_else(|| Status::invalid_argument("points required"))?;

        let colors = crate::colormap::apply_colormap(&points.positions, req.min_z, req.max_z);

        Ok(Response::new(pb::ApplyColormapResponse {
            points: Some(pb::PointCloudChunk {
                positions: points.positions,
                colors,
                normals: vec![],
            }),
        }))
    }
}
