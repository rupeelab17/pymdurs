//! Shared IGN WFS layer collector (Cadastre / Road / Iris).

use anyhow::{Context, Result};
use geojson::GeoJson;
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// Which IGN layer [`IgnLayer`] downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnLayerKind {
    Cadastre,
    Road,
    Iris,
}

impl IgnLayerKind {
    /// Key passed to [`IgnCollect::execute_ign`].
    pub fn execute_key(self) -> &'static str {
        match self {
            Self::Cadastre => "cadastre",
            Self::Road => "road",
            Self::Iris => "iris",
        }
    }

    /// Default basename for [`IgnLayer::to_geojson`].
    pub fn default_output_name(self) -> &'static str {
        match self {
            Self::Cadastre => "cadastre",
            Self::Road => "road",
            Self::Iris => "iris",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cadastre => "Cadastre",
            Self::Road => "Road",
            Self::Iris => "Iris",
        }
    }
}

/// Generic IGN GeoJSON layer: download via WFS, parse, optional save.
pub struct IgnLayer {
    kind: IgnLayerKind,
    ign_collect: IgnCollect,
    output_path: PathBuf,
    pub geo_core: GeoCore,
    bbox: Option<BoundingBox>,
    geojson: Option<GeoJson>,
}

impl IgnLayer {
    /// Create a layer collector for `kind`.
    pub fn new(kind: IgnLayerKind, output_path: Option<String>) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);

        Ok(Self {
            kind,
            ign_collect: IgnCollect::new()?,
            output_path: output_path_buf,
            geo_core: GeoCore::default(),
            bbox: None,
            geojson: None,
        })
    }

    pub fn kind(&self) -> IgnLayerKind {
        self.kind
    }

    pub fn bbox(&self) -> Option<&BoundingBox> {
        self.bbox.as_ref()
    }

    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.ign_collect.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
    }

    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core.set_epsg(epsg);
        self.ign_collect.geo_core.set_epsg(epsg);
    }

    pub fn run(mut self) -> Result<Self> {
        self.run_internal()?;
        Ok(self)
    }

    pub fn run_internal(&mut self) -> Result<()> {
        let key = self.kind.execute_key();
        self.ign_collect
            .execute_ign(key)
            .with_context(|| format!("Failed to execute IGN request for {key}"))?;

        let content = self
            .ign_collect
            .content
            .as_ref()
            .context("No content received from IGN API")?;

        let geojson: GeoJson = String::from_utf8_lossy(content)
            .parse()
            .context("Failed to parse GeoJSON from IGN API response")?;

        self.geojson = Some(geojson);
        Ok(())
    }

    pub fn get_geojson(&self) -> Option<&GeoJson> {
        self.geojson.as_ref()
    }

    pub fn to_geojson(&self, name: Option<&str>) -> Result<()> {
        let geojson = self
            .geojson
            .as_ref()
            .context("No GeoJSON data available. Call run() first.")?;

        let name = name.unwrap_or(self.kind.default_output_name());
        let output_file = self.output_path.join(format!("{name}.geojson"));
        std::fs::write(&output_file, geojson.to_string())
            .with_context(|| format!("Failed to write GeoJSON file: {output_file:?}"))?;

        tracing::info!(
            "{} saved to: {:?} (as GeoJSON - GeoJSON export temporarily disabled)",
            self.kind.label(),
            output_file
        );
        Ok(())
    }

    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_keys_and_default_names() {
        assert_eq!(IgnLayerKind::Cadastre.execute_key(), "cadastre");
        assert_eq!(IgnLayerKind::Road.execute_key(), "road");
        assert_eq!(IgnLayerKind::Iris.execute_key(), "iris");
        assert_eq!(IgnLayerKind::Cadastre.default_output_name(), "cadastre");
        assert_eq!(IgnLayerKind::Road.default_output_name(), "road");
        assert_eq!(IgnLayerKind::Iris.default_output_name(), "iris");
    }

    #[test]
    fn each_kind_new_bbox_crs() {
        for kind in [
            IgnLayerKind::Cadastre,
            IgnLayerKind::Road,
            IgnLayerKind::Iris,
        ] {
            let mut layer = IgnLayer::new(kind, None).unwrap();
            assert_eq!(layer.kind(), kind);
            assert!(!layer.get_output_path().as_os_str().is_empty());
            assert_eq!(layer.geo_core.get_epsg(), 2154);

            layer.set_bbox(-1.15, 46.18, -1.14, 46.19);
            layer.set_crs(2154);
            assert!(layer.bbox().is_some());
            assert_eq!(layer.geo_core.get_epsg(), 2154);
        }
    }

    #[test]
    fn to_geojson_requires_run() {
        let layer = IgnLayer::new(IgnLayerKind::Cadastre, None).unwrap();
        let err = layer.to_geojson(None).unwrap_err().to_string();
        assert!(err.contains("Call run() first"));
    }

    #[test]
    fn to_geojson_uses_kind_default_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut layer = IgnLayer::new(
            IgnLayerKind::Road,
            Some(dir.path().to_string_lossy().into()),
        )
        .unwrap();
        layer.geojson = Some(GeoJson::FeatureCollection(geojson::FeatureCollection {
            bbox: None,
            features: vec![],
            foreign_members: None,
        }));
        layer.to_geojson(None).unwrap();
        assert!(dir.path().join("road.geojson").is_file());
    }
}
