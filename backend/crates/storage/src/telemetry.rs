use excalibur_domain::{Id, TelemetryPoint};

pub(crate) type TelemetryDedupeKey = (Id, Id, String, i64);

pub(crate) fn telemetry_dedupe_key(point: &TelemetryPoint) -> TelemetryDedupeKey {
    (
        point.project_id,
        point.device_id,
        point.stream.clone(),
        point.sequence,
    )
}
