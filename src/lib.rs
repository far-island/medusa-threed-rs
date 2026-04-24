//! Library surface of the `medusa-threed-rs` crate.
//!
//! The crate ships both as a standalone binary (`medusa-threed-rs`, see
//! `src/main.rs`) and as a library that downstream Rust consumers —
//! primarily `medusa-ai`'s `medusa-gateway` — can link against to host
//! the gRPC services in-process without spawning the standalone binary.
//!
//! # Public surface
//!
//! The primary export is [`service::PointCloudServiceImpl`], which
//! implements the generated `PointCloudService` trait from the
//! `farisland.threed.v1` protobuf package. The sibling service impls
//! (`MeshServiceImpl`, `ThreeDScanServiceImpl`,
//! `ScaleConfigurationServiceImpl`) are exported from their respective
//! modules for consumers that want a unified gateway binary.
//!
//! Low-level compute helpers (`pcd`, `colormap`, `delaunator`,
//! `grid_mesher`, `resample`, `scan`, `statistics`) are exposed too so
//! that consumers can reuse them without going through the gRPC
//! boundary when they already hold the inputs as Rust types (e.g. the
//! gateway composing multiple operations in a single request).
//!
//! # Stability
//!
//! This surface is intentionally unstable at 0.x — re-exports may be
//! tightened or moved behind feature flags as the gateway integration
//! solidifies. Consumers should pin the crate version.

pub mod colormap;
pub mod delaunator;
pub mod grid_mesher;
pub mod mesh_service;
pub mod pcd;
pub mod proto;
pub mod resample;
pub mod scale_configuration_service;
pub mod scan;
pub mod scan_service;
pub mod service;
pub mod statistics;
