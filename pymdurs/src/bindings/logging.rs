//! Expose `rsmdu::logging::init` to Python.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3_stub_gen::derive::gen_stub_pyfunction;

/// Install Rust tracing on stderr. Idempotent. Default silent until called.
///
/// `level` is a filter directive (`"info"`, `"rsmdu=debug"`, `"rsmdu::lidar_timing=info"`).
/// `RUST_LOG` wins when set.
#[gen_stub_pyfunction(module = "pymdurs")]
#[pyfunction]
#[pyo3(signature = (level = "info"))]
pub fn init_logging(level: &str) -> PyResult<()> {
    rsmdu::logging::init(level).map_err(PyRuntimeError::new_err)
}
