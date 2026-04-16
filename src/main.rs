mod colormap;
mod pcd;
mod proto;
mod resample;
mod service;
mod statistics;

use proto::farisland::threed::v1::point_cloud_service_server::PointCloudServiceServer;
use service::PointCloudServiceImpl;
use tonic::transport::Server;

/// Rust-side gRPC port (as defined in STRATEGY.md).
const RUST_GRPC_PORT: u16 = 50052;

/// Java core gRPC port (for module registration).
const _JAVA_GRPC_PORT: u16 = 50051;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("0.0.0.0:{RUST_GRPC_PORT}").parse()?;

    println!("[medusa-threed-rs] Starting PointCloudService on {addr}");

    // TODO: Register with Java core via ModuleRegistryService.RegisterModule
    // on :50051 once the Java gRPC adapter is running.
    // For now, just start the server.

    Server::builder()
        .add_service(PointCloudServiceServer::new(PointCloudServiceImpl))
        .serve(addr)
        .await?;

    Ok(())
}
