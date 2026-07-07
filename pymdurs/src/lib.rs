use pyo3::prelude::*;

mod bindings;

use bindings::{
    PyBoundingBox, PyBuilding, PyCadastre, PyCosia, PyDem, PyGeoCore, PyIris, PyLcz, PyLidar, PyMnh,
    PyMns, PyMnt, PyRnb, PyRoad, PyVegetation, PyWater, PyWindConfig, PyWindField,
};

/// Python bindings for pymdurs
/// Rust transpilation of pymdu (Python Urban Data Model)

#[pymodule]
fn pymdurs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register submodules
    register_geometric_module(m)?;
    register_thermal_module(m)?;

    // Register core classes
    m.add_class::<PyBoundingBox>()?;
    m.add_class::<PyGeoCore>()?;
    // Add aliases for Pythonic API
    m.setattr("BoundingBox", m.getattr("PyBoundingBox")?)?;
    m.setattr("GeoCore", m.getattr("PyGeoCore")?)?;

    m.add(
        "__doc__",
        "Python bindings for pymdurs - Rust transpilation of pymdu (Python Urban Data Model)",
    )?;

    Ok(())
}

fn register_geometric_module(py_module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = py_module.py();
    let submodule = PyModule::new(py, "geometric")?;
    submodule.add("__doc__", "Geometric data processing classes.")?;

    // Register all geometric classes
    submodule.add_class::<PyBuilding>()?;
    submodule.add_class::<PyCadastre>()?;
    submodule.add_class::<PyCosia>()?;
    submodule.add_class::<PyDem>()?;
    submodule.add_class::<PyIris>()?;
    submodule.add_class::<PyLcz>()?;
    submodule.add_class::<PyLidar>()?;
    submodule.add_class::<PyMnh>()?;
    submodule.add_class::<PyMns>()?;
    submodule.add_class::<PyMnt>()?;
    submodule.add_class::<PyRoad>()?;
    submodule.add_class::<PyRnb>()?;
    submodule.add_class::<PyVegetation>()?;
    submodule.add_class::<PyWater>()?;

    // Add aliases for Pythonic API (Building instead of PyBuilding)
    submodule.setattr("Building", submodule.getattr("PyBuilding")?)?;
    submodule.setattr("Cadastre", submodule.getattr("PyCadastre")?)?;
    submodule.setattr("Cosia", submodule.getattr("PyCosia")?)?;
    submodule.setattr("Dem", submodule.getattr("PyDem")?)?;
    submodule.setattr("Iris", submodule.getattr("PyIris")?)?;
    submodule.setattr("Lcz", submodule.getattr("PyLcz")?)?;
    submodule.setattr("Lidar", submodule.getattr("PyLidar")?)?;
    submodule.setattr("Mnh", submodule.getattr("PyMnh")?)?;
    submodule.setattr("Mns", submodule.getattr("PyMns")?)?;
    submodule.setattr("Mnt", submodule.getattr("PyMnt")?)?;
    submodule.setattr("Road", submodule.getattr("PyRoad")?)?;
    submodule.setattr("Rnb", submodule.getattr("PyRnb")?)?;
    submodule.setattr("Vegetation", submodule.getattr("PyVegetation")?)?;
    submodule.setattr("Water", submodule.getattr("PyWater")?)?;

    // CRITICAL: Register in sys.modules with full name FIRST
    /*let sys = py.import("sys")?;
    let modules_attr = sys.getattr("modules")?;
    let modules = modules_attr.cast::<PyDict>()?;
    modules.set_item("pymdurs.geometric", &submodule)?;*/

    // Then add to parent module - this makes it accessible as pymdurs.geometric
    py_module.add_submodule(&submodule)?;

    Ok(())
}

fn register_thermal_module(py_module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = py_module.py();
    let submodule = PyModule::new(py, "thermal")?;
    submodule.add("__doc__", "Urban wind field (Röckle model) for UTCI pipeline.")?;
    submodule.add_class::<PyWindField>()?;
    submodule.add_class::<PyWindConfig>()?;
    submodule.setattr("WindField", submodule.getattr("PyWindField")?)?;
    submodule.setattr("WindConfig", submodule.getattr("PyWindConfig")?)?;
    py_module.add_submodule(&submodule)?;
    Ok(())
}

// Public API aliases (runtime setattr in register_*_module)
pyo3_stub_gen::type_alias!("pymdurs", BoundingBox = PyBoundingBox);
pyo3_stub_gen::type_alias!("pymdurs", GeoCore = PyGeoCore);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Building = PyBuilding);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Cadastre = PyCadastre);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Cosia = PyCosia);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Dem = PyDem);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Iris = PyIris);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Lcz = PyLcz);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Lidar = PyLidar);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Mnh = PyMnh);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Mns = PyMns);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Mnt = PyMnt);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Road = PyRoad);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Rnb = PyRnb);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Vegetation = PyVegetation);
pyo3_stub_gen::type_alias!("pymdurs.geometric", Water = PyWater);
pyo3_stub_gen::type_alias!("pymdurs.thermal", WindField = PyWindField);
pyo3_stub_gen::type_alias!("pymdurs.thermal", WindConfig = PyWindConfig);

/// Gather stub metadata from the workspace `pyproject.toml` (one level above `Cargo.toml`).
pub fn stub_info() -> pyo3_stub_gen::Result<pyo3_stub_gen::StubInfo> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut stub =
        pyo3_stub_gen::StubInfo::from_pyproject_toml(manifest_dir.join("../pyproject.toml"))?;
    // Mixed layout: Python package and Rust extension live in `pymdurs/`.
    stub.is_mixed_layout = true;
    stub.python_root = manifest_dir.to_path_buf();
    Ok(stub)
}
