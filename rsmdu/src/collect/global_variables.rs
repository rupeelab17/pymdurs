use std::path::PathBuf;
use std::sync::OnceLock;

/// Scratch dir: `$PYMDURS_TEMP` or `{temp_dir}/pymdurs-{pid}`.
pub fn get_temp_path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let pb = std::env::var_os("PYMDURS_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("pymdurs-{}", std::process::id()))
            });
        let _ = std::fs::create_dir_all(&pb);
        pb
    })
    .clone()
}
