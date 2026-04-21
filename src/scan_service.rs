// ThreeDScanService gRPC implementation.
//
// ScanAll/ScanOne call Java MetrologyCallbackService on :50051 for
// profile detection, then convert profiles to 3D points in Rust.
// ListDatasets is pure filesystem I/O.

use std::path::Path;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::proto::farisland::threed::v1::{
    self as pb,
    metrology_callback_service_client::MetrologyCallbackServiceClient,
    three_d_scan_service_server::ThreeDScanService,
};

/// Java core gRPC endpoint for metrology callback.
const JAVA_ENDPOINT: &str = "http://127.0.0.1:50051";

pub struct ThreeDScanServiceImpl {
    /// Lazy-initialized client to Java MetrologyCallbackService.
    metrology_client:
        Mutex<Option<MetrologyCallbackServiceClient<tonic::transport::Channel>>>,
}

impl ThreeDScanServiceImpl {
    pub fn new() -> Self {
        Self {
            metrology_client: Mutex::new(None),
        }
    }

    /// Get or create the metrology callback client.
    async fn get_metrology_client(
        &self,
    ) -> Result<MetrologyCallbackServiceClient<tonic::transport::Channel>, Status> {
        let mut guard = self.metrology_client.lock().await;
        if let Some(ref client) = *guard {
            return Ok(client.clone());
        }

        let client = MetrologyCallbackServiceClient::connect(JAVA_ENDPOINT)
            .await
            .map_err(|e| {
                Status::unavailable(format!(
                    "Cannot connect to Java MetrologyCallbackService at {JAVA_ENDPOINT}: {e}"
                ))
            })?;

        *guard = Some(client.clone());
        Ok(client)
    }

    /// Call Java to detect a profile, then convert to 3D points.
    async fn scan_single_slice(
        &self,
        image_path: &str,
        angle: f64,
        decimation: i32,
        pixel_density_horizontal: f64,
        pixel_density_vertical: f64,
        strategy: i32,
    ) -> Result<crate::scan::ProfilePoints, Status> {
        let mut client = self.get_metrology_client().await?;

        let resp = client
            .detect_profile(pb::DetectProfileRequest {
                image_path: image_path.to_string(),
                decimation,
                strategy,
            })
            .await?
            .into_inner();

        if !resp.success {
            return Err(Status::internal(format!(
                "Detection failed for {image_path}: {}",
                resp.error_message
            )));
        }

        Ok(crate::scan::profile_to_points(
            &resp.left_upper_profile,
            &resp.right_lower_profile,
            resp.step,
            resp.scan_area_height,
            angle,
            pixel_density_horizontal,
            pixel_density_vertical,
        ))
    }
}

#[tonic::async_trait]
impl ThreeDScanService for ThreeDScanServiceImpl {
    type ScanAllStream = tokio_stream::wrappers::ReceiverStream<Result<pb::ScanProgress, Status>>;

    async fn scan_all(
        &self,
        request: Request<pb::ScanAllRequest>,
    ) -> Result<Response<Self::ScanAllStream>, Status> {
        let req = request.into_inner();
        let dataset_path = req.dataset_path.clone();
        let decimation = req.decimation;
        let pdh = req.pixel_density_horizontal;
        let pdv = req.pixel_density_vertical;
        let strategy = req.strategy;

        // The Java client passes an absolute directory path whose direct
        // children are `<angle>.png` slice files (legacy ThreeDController
        // dataset shape). Enumerate that directory directly — no parent
        // rescan.
        let slices = crate::scan::list_slices_in_dir(Path::new(&dataset_path));

        let slices_total = slices.len() as i32;
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<pb::ScanProgress, Status>>(32);

        // We need a reference to self that's 'static for the spawned task.
        // Clone what we need instead.
        let metrology_client = self.get_metrology_client().await.ok();

        tokio::spawn(async move {
            let client_mutex = Mutex::new(metrology_client);

            for (idx, slice) in slices.into_iter().enumerate() {
                // Client cancellation: when the JavaFX consumer drops the
                // stream observer (panel dispose / user navigates away),
                // the mpsc receiver closes. Skip remaining slices instead
                // of continuing to hammer the Java callback server.
                if tx.is_closed() {
                    break;
                }
                let result = {
                    let mut guard = client_mutex.lock().await;
                    let client = match guard.as_mut() {
                        Some(c) => c,
                        None => {
                            let _ = tx
                                .send(Err(Status::unavailable("No metrology client")))
                                .await;
                            return;
                        }
                    };

                    client
                        .detect_profile(pb::DetectProfileRequest {
                            image_path: slice.path.clone(),
                            decimation,
                            strategy,
                        })
                        .await
                };

                let progress = match result {
                    Ok(resp) => {
                        let resp = resp.into_inner();
                        // Both success and failure emit a ScanProgress for
                        // this slice index. The failure case yields an
                        // empty PointCloudChunk so the client can keep
                        // slice_index contiguous against slices_total
                        // (progress bars, slice counters).
                        let pts = if resp.success {
                            crate::scan::profile_to_points(
                                &resp.left_upper_profile,
                                &resp.right_lower_profile,
                                resp.step,
                                resp.scan_area_height,
                                slice.angle,
                                pdh,
                                pdv,
                            )
                        } else {
                            crate::scan::ProfilePoints {
                                positions: Vec::new(),
                                colors: Vec::new(),
                            }
                        };

                        Ok(pb::ScanProgress {
                            slice_index: idx as i32,
                            slices_total,
                            slice_angle: slice.angle,
                            points: Some(pb::PointCloudChunk {
                                positions: pts.positions,
                                colors: pts.colors,
                                normals: Vec::new(),
                            }),
                        })
                    }
                    Err(e) => Err(Status::internal(format!(
                        "Detection failed for slice {}: {e}",
                        slice.filename
                    ))),
                };

                let is_fatal = progress.is_err();
                if tx.send(progress).await.is_err() {
                    break; // client disconnected mid-send
                }
                if is_fatal {
                    // Trailing Status is terminal in gRPC semantics — do not
                    // keep emitting ScanProgress after an Err was sent.
                    break;
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn scan_one(
        &self,
        request: Request<pb::ScanOneRequest>,
    ) -> Result<Response<pb::ScanOneResponse>, Status> {
        let req = request.into_inner();

        let pts = self
            .scan_single_slice(
                &req.slice_path,
                req.angle,
                1, // decimation=1 for single slice, matches Java scanOne
                req.pixel_density_horizontal,
                req.pixel_density_vertical,
                req.strategy,
            )
            .await?;

        Ok(Response::new(pb::ScanOneResponse {
            points: Some(pb::PointCloudChunk {
                positions: pts.positions,
                colors: pts.colors,
                normals: vec![],
            }),
        }))
    }

    async fn list_datasets(
        &self,
        request: Request<pb::ListDatasetsRequest>,
    ) -> Result<Response<pb::ListDatasetsResponse>, Status> {
        let req = request.into_inner();
        let root = if req.root_path.is_empty() {
            return Err(Status::invalid_argument("root_path required"));
        } else {
            req.root_path
        };

        let datasets = crate::scan::list_datasets(Path::new(&root));

        let response = pb::ListDatasetsResponse {
            datasets: datasets
                .into_iter()
                .map(|ds| pb::DatasetInfo {
                    name: ds.name,
                    path: ds.path,
                    slice_count: ds.slices.len() as i32,
                    slices: ds
                        .slices
                        .into_iter()
                        .map(|s| pb::SliceInfo {
                            filename: s.filename,
                            path: s.path,
                            angle: s.angle,
                        })
                        .collect(),
                })
                .collect(),
        };

        Ok(Response::new(response))
    }
}
