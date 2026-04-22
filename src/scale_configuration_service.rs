// ScaleConfigurationService gRPC implementation (Phase 3 LIVE).
//
// Stateless per-chunk running-max aggregation + effective-scale policy.
// See farisland/threed/v1/scale_configuration.proto for the wire contract.
//
// Semantics are under core review as of 2026-04-22 — this module is the
// transport scaffolding only.  The compute path lives behind
// `compute_scale_policy` which is the single place to revisit once the
// body-field semantics are finalised (raw_scale_factor interpretation,
// maxAbsCoord policy location, aggregation strategy).

use tonic::{Request, Response, Status};

use crate::proto::farisland::threed::v1::{
    self as pb,
    scale_configuration_service_server::ScaleConfigurationService,
};

pub struct ScaleConfigurationServiceImpl {
    /// Normalisation target — the coordinate magnitude every cloud is
    /// scaled to match.  Server-side policy per the 2026-04-22 shape
    /// decision; clients derive it as `effective_scale * new_running_max`
    /// if they need the value.  Default mirrors the legacy Java
    /// `MAX_ABS_COORDINATE` constant in PCDViewer.
    max_abs_coord: f64,
}

impl ScaleConfigurationServiceImpl {
    pub fn new(max_abs_coord: f64) -> Self {
        Self { max_abs_coord }
    }
}

impl Default for ScaleConfigurationServiceImpl {
    fn default() -> Self {
        // Matches legacy PCDViewer MAX_ABS_COORDINATE.  Externalised to
        // a config source once the server policy layer is in place.
        Self::new(1000.0)
    }
}

#[tonic::async_trait]
impl ScaleConfigurationService for ScaleConfigurationServiceImpl {
    async fn compute_scale(
        &self,
        request: Request<pb::ComputeScaleRequest>,
    ) -> Result<Response<pb::ComputeScaleResponse>, Status> {
        let req = request.into_inner();
        if !req.raw_scale_factor.is_finite() || req.raw_scale_factor < 0.0 {
            return Err(Status::invalid_argument(
                "raw_scale_factor must be finite and non-negative",
            ));
        }
        if !req.running_max.is_finite() || req.running_max < 0.0 {
            return Err(Status::invalid_argument(
                "running_max must be finite and non-negative",
            ));
        }

        let (new_running_max, effective_scale) =
            compute_scale_policy(req.raw_scale_factor, req.running_max, self.max_abs_coord);

        Ok(Response::new(pb::ComputeScaleResponse {
            new_running_max,
            effective_scale,
        }))
    }
}

/// Aggregation + effective-scale policy.
///
/// Today: running max with `effective_scale = max_abs_coord / new_running_max`.
/// Candidate future policies (EMA, percentile, hysteresis) plug in here
/// without touching the RPC boundary.  Returns `(new_running_max, effective_scale)`.
fn compute_scale_policy(chunk_contribution: f64, running_max: f64, max_abs_coord: f64) -> (f64, f64) {
    let new_running_max = chunk_contribution.max(running_max);
    let effective_scale = if new_running_max > 0.0 {
        max_abs_coord / new_running_max
    } else {
        0.0
    };
    (new_running_max, effective_scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_call_running_max_zero_sets_it_to_chunk() {
        let (running, scale) = compute_scale_policy(7.5, 0.0, 1000.0);
        assert_eq!(running, 7.5);
        assert!((scale - 1000.0 / 7.5).abs() < 1e-12);
    }

    #[test]
    fn chunk_smaller_than_running_keeps_running() {
        let (running, scale) = compute_scale_policy(3.0, 10.0, 1000.0);
        assert_eq!(running, 10.0);
        assert!((scale - 100.0).abs() < 1e-12);
    }

    #[test]
    fn chunk_larger_than_running_replaces_it() {
        let (running, scale) = compute_scale_policy(20.0, 10.0, 1000.0);
        assert_eq!(running, 20.0);
        assert!((scale - 50.0).abs() < 1e-12);
    }

    #[test]
    fn zero_running_and_zero_chunk_returns_zero_scale() {
        let (running, scale) = compute_scale_policy(0.0, 0.0, 1000.0);
        assert_eq!(running, 0.0);
        assert_eq!(scale, 0.0);
    }
}
