//! Tracing subscriber init — silent until called (or `RUST_LOG` via [`init`]).

/// Install a stderr `tracing` subscriber. Idempotent.
///
/// `level` is an [`EnvFilter`](tracing_subscriber::EnvFilter) directive
/// (e.g. `"info"`, `"rsmdu=debug"`, `"rsmdu::lidar_timing=info"`).
/// Prefer `RUST_LOG` when set.
pub fn init(level: &str) -> Result<(), String> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new(level))
        .map_err(|e| e.to_string())?;
    // try_init: Ok on first call, Err if already installed — both fine.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .try_init();
    Ok(())
}
