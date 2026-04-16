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
use tower_http::cors::{Any, CorsLayer};
use http::header::HeaderName;

/// Rust-side gRPC port (as defined in STRATEGY.md).
const RUST_GRPC_PORT: u16 = 50052;

/// Java core gRPC port (for module registration).
const _JAVA_GRPC_PORT: u16 = 50051;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("0.0.0.0:{RUST_GRPC_PORT}").parse()?;

    println!("[medusa-threed-rs] Starting gRPC + gRPC-web server on {addr}");

    // CORS layer for gRPC-web from browser WebView (file:// or localhost origins).
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
        .expose_headers([
            HeaderName::from_static("grpc-status"),
            HeaderName::from_static("grpc-message"),
        ]);

    Server::builder()
        .accept_http1(true) // required for gRPC-web (uses HTTP/1.1)
        .layer(cors)
        .add_service(tonic_web::enable(PointCloudServiceServer::new(PointCloudServiceImpl)))
        .add_service(tonic_web::enable(MeshServiceServer::new(MeshServiceImpl)))
        .add_service(tonic_web::enable(ThreeDScanServiceServer::new(ThreeDScanServiceImpl::new())))
        .serve(addr)
        .await?;

    Ok(())
}
