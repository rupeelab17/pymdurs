fn main() -> pyo3_stub_gen::Result<()> {
    pymdurs::stub_info()?.generate()?;
    Ok(())
}
