/// Convert `SystemTime` to ISO 8601 string (e.g. `2026-07-05T21:18:20.000Z`).
pub fn system_time_to_iso(t: std::time::SystemTime) -> Option<String> {
    let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    let naive = chrono::DateTime::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos())?;
    Some(naive.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}
