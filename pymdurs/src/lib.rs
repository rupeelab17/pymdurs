use pyo3::prelude::*;

mod bindings;

use bindings::{
    PyBoundingBox, PyBuilding, PyCadastre, PyCosia, PyDem, PyGeoCore, PyIris, PyLcz, PyLidar, PyRnb,
    PyRoad, PyVegetation, PyWater,
};

/// Python bindings for pymdurs
/// Rust transpilation of pymdu (Python Urban Data Model)

#[pymodule]
fn pymdurs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register submodules
    register_geometric_module(m)?;

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
