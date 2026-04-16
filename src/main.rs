mod colormap;
mod delaunator;
mod grid_mesher;
mod mesh_service;
mod pcd;
mod proto;
mod resample;
mod scan;
mod scan_service;
mod service;
mod statistics;

use proto::farisland::threed::v1::mesh_service_server::MeshServiceServer;
use proto::farisland::threed::v1::point_cloud_service_server::PointCloudServiceServer;
use proto::farisland::threed::v1::three_d_scan_service_server::ThreeDScanServiceServer;
use mesh_service::MeshServiceImpl;
use scan_service::ThreeDScanServiceImpl;
use service::PointCloudServiceImpl;
use tonic::transport::Server;

/// Rust-side gRPC port (as defined in STRATEGY.md).
const RUST_GRPC_PORT: u16 = 50052;

/// Java core gRPC port (for module registration).
const _JAVA_GRPC_PORT: u16 = 50051;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("0.0.0.0:{RUST_GRPC_PORT}").parse()?;

    println!("[medusa-threed-rs] Starting PointCloudService + MeshService + ThreeDScanService on {addr}");

    // TODO: Register with Java core via ModuleRegistryService.RegisterModule
    // on :50051 once the Java gRPC adapter is running.
    // For now, just start the server.

    Server::builder()
        .add_service(PointCloudServiceServer::new(PointCloudServiceImpl))
        .add_service(MeshServiceServer::new(MeshServiceImpl))
        .add_service(ThreeDScanServiceServer::new(ThreeDScanServiceImpl::new()))
        .serve(addr)
        .await?;

    Ok(())
}
