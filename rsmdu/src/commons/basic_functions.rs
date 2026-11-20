use anyhow::{Context, Result};
use geojson::GeoJson;
use serde::{Deserialize, Serialize};
use std::{path::Path, process::Command};

pub(crate) fn shapefile_to_geojson(filepath: &Path, prefix: &str) -> Result<GeoJson> {
    // GeoJSON driver refuses overwrite: reserve a path then remove the empty tempfile.
    let tmp = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".geojson")
        .tempfile()
        .context("Failed to create temporary GeoJSON file")?;
    let temp_geojson = tmp.path().to_path_buf();
    drop(tmp);

    let status = Command::new("ogr2ogr")
        .args(["-f", "GeoJSON"])
        .arg(&temp_geojson)
        .arg(filepath)
        .status()
        .context("Failed to execute ogr2ogr. Make sure GDAL is installed and ogr2ogr is in PATH")?;
    if !status.success() {
        let _ = std::fs::remove_file(&temp_geojson);
        anyhow::bail!("ogr2ogr failed to convert shapefile to GeoJSON");
    }

    let bytes = std::fs::read(&temp_geojson).context("Failed to read temporary GeoJSON file")?;
    let _ = std::fs::remove_file(&temp_geojson);
    String::from_utf8_lossy(&bytes)
        .parse()
        .context("Failed to parse GeoJSON from shapefile")
}

/// Process datetime fields in a dataset
/// This is a placeholder for datetime processing functionality
pub fn process_datetime<T: Serialize + for<'de> Deserialize<'de>>(_data: T) -> T {
    // TODO: Implement datetime processing logic
    // For now, just return the data as-is
    _data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapefile_geojson_roundtrip() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("source.geojson");
        let shapefile = dir.path().join("source.shp");
        std::fs::write(
            &source,
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"NDVI":0},"geometry":{"type":"Point","coordinates":[1,2]}}]}"#,
        )?;
        let status = Command::new("ogr2ogr")
            .args(["-f", "ESRI Shapefile"])
            .arg(&shapefile)
            .arg(&source)
            .status()?;
        assert!(
            status.success(),
            "failed to create test shapefile: {status}"
        );
        let expected: GeoJson = std::fs::read_to_string(&source)?.parse()?;
        let actual = shapefile_to_geojson(&shapefile, "roundtrip_")?;
        let (GeoJson::FeatureCollection(expected), GeoJson::FeatureCollection(actual)) =
            (expected, actual)
        else {
            panic!("expected feature collections");
        };
        assert_eq!(actual.features.len(), 1);
        assert_eq!(actual.features[0].geometry, expected.features[0].geometry);
        assert_eq!(
            actual.features[0].properties,
            expected.features[0].properties
        );
        assert!(
            shapefile_to_geojson(&dir.path().join("missing.shp"), "missing_")
                .unwrap_err()
                .to_string()
                .contains("ogr2ogr failed")
        );
        Ok(())
    }

    #[test]
    fn test_process_datetime_roundtrip() {
        assert_eq!(process_datetime(42u32), 42);
    }
}
