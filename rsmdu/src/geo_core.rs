use anyhow::{Context, Result};
use geo::Point;
use proj::Proj;

/// Base struct for geospatial operations
/// Following Python: class GeoCore
/// Handles CRS (Coordinate Reference System), bounding box, and output paths
#[derive(Clone)]
pub struct GeoCore {
    /// EPSG code (Python: _epsg)
    pub epsg: i32,
    /// Bounding box (Python: _Bbox)
    pub bbox: Option<BoundingBox>,
    /// Output path for processed data (Python: _output_path)
    pub output_path: Option<String>,
    /// Output path for shapefile (Python: _output_path_shp)
    pub output_path_shp: Option<String>,
    /// Filename for shapefile (Python: _filename_shp)
    pub filename_shp: Option<String>,
}

impl Default for GeoCore {
    /// EPSG:2154 (Lambert-93).
    fn default() -> Self {
        Self::new(2154)
    }
}

impl GeoCore {
    /// Create a new GeoCore with EPSG
    /// Following Python: _epsg: int = 2154
    pub fn new(epsg: i32) -> Self {
        GeoCore {
            epsg,
            bbox: None,
            output_path: None,
            output_path_shp: None,
            filename_shp: None,
        }
    }

    /// Get EPSG code
    /// Following Python: @property def epsg(self): return self._epsg
    pub fn get_epsg(&self) -> i32 {
        self.epsg
    }

    /// Set EPSG code
    /// Following Python: @epsg.setter def epsg(self, value): self._epsg = value
    pub fn set_epsg(&mut self, epsg: i32) {
        self.epsg = epsg;
    }

    /// Get bounding box
    /// Following Python: @classproperty def Bbox(cls): return cls._Bbox
    pub fn get_bbox(&self) -> Option<BoundingBox> {
        self.bbox
    }

    /// Set bounding box
    /// Following Python: @Bbox.setter def Bbox(cls, value): cls._Bbox = value
    pub fn set_bbox(&mut self, bbox: Option<BoundingBox>) {
        self.bbox = bbox;
    }

    /// Get output path
    /// Following Python: @property def output_path(self): return self._output_path
    pub fn get_output_path(&self) -> Option<&String> {
        self.output_path.as_ref()
    }

    /// Set output path
    /// Following Python: @output_path.setter def output_path(self, value): self._output_path = value
    pub fn set_output_path(&mut self, output_path: Option<String>) {
        self.output_path = output_path;
    }

    /// Get output path for shapefile
    /// Following Python: @property def output_path_shp(self): return self._output_path_shp
    pub fn get_output_path_shp(&self) -> Option<&String> {
        self.output_path_shp.as_ref()
    }

    /// Set output path for shapefile
    /// Following Python: @output_path_shp.setter def output_path_shp(self, value): self._output_path_shp = value
    pub fn set_output_path_shp(&mut self, output_path_shp: Option<String>) {
        self.output_path_shp = output_path_shp;
    }

    /// Get filename for shapefile
    /// Following Python: @property def filename_shp(self): return self._filename_shp
    pub fn get_filename_shp(&self) -> Option<&String> {
        self.filename_shp.as_ref()
    }

    /// Set filename for shapefile
    /// Following Python: @filename_shp.setter def filename_shp(self, value): self._filename_shp = value
    pub fn set_filename_shp(&mut self, filename_shp: Option<String>) {
        self.filename_shp = filename_shp;
    }

    /// Transform coordinates from one CRS to another
    pub fn transform_coords(from_epsg: i32, to_epsg: i32, x: f64, y: f64) -> Result<(f64, f64)> {
        let from_crs = format!("EPSG:{}", from_epsg);
        let to_crs = format!("EPSG:{}", to_epsg);

        let proj = Proj::new_known_crs(&from_crs, &to_crs, None)
            .context("Failed to create Proj transformation")?;

        let result = proj
            .convert((x, y))
            .context("Failed to transform coordinates")?;

        Ok(result)
    }

    /// Transform a Point from one CRS to another
    pub fn transform_point(from_epsg: i32, to_epsg: i32, point: Point<f64>) -> Result<Point<f64>> {
        let (x, y) = Self::transform_coords(from_epsg, to_epsg, point.x(), point.y())?;
        Ok(Point::new(x, y))
    }

    /// Get a Proj instance for this CRS
    pub fn get_proj(&self) -> Result<Proj> {
        let crs = format!("EPSG:{}", self.epsg);
        Proj::new_known_crs(&crs, &crs, None).context("Failed to create Proj instance")
    }
}

/// Bounding box structure
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min_x: f64, // min longitude
    pub min_y: f64, // min latitude
    pub max_x: f64, // max longitude
    pub max_y: f64, // max latitude
}

impl BoundingBox {
    pub fn new(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        BoundingBox {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Transform bounds using all corners and densified edges to account for projection curvature.
    pub fn transform(&self, from_epsg: i32, to_epsg: i32) -> Result<Self> {
        let proj = Proj::new_known_crs(
            &format!("EPSG:{from_epsg}"),
            &format!("EPSG:{to_epsg}"),
            None,
        )
        .context("Failed to create Proj transformation")?;
        // PROJ's recommended densification: 21 additional points per edge.
        const DENSIFY_POINTS: i32 = 21;
        let [min_x, min_y, max_x, max_y] = proj
            .transform_bounds(
                self.min_x,
                self.min_y,
                self.max_x,
                self.max_y,
                DENSIFY_POINTS,
            )
            .context("Failed to transform bounding box")?;

        Ok(BoundingBox::new(min_x, min_y, max_x, max_y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geo_core_default() {
        let gc = GeoCore::default();
        assert_eq!(gc.get_epsg(), 2154);
    }

    #[test]
    fn test_bounding_box() {
        let bbox: BoundingBox = BoundingBox::new(0.0, 0.0, 1.0, 1.0);
        assert_eq!(bbox.min_x, 0.0);
        assert_eq!(bbox.max_x, 1.0);
    }

    #[test]
    fn test_bounding_box_transform_reunion() -> Result<()> {
        let bbox = BoundingBox::new(55.51, -21.285, 55.53, -21.265);
        let transformed = bbox.transform(4326, 2154)?;
        let proj = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)?;

        // Check all four corners and samples along every edge, not just SW/NE.
        for step in 0..=22 {
            let fraction = f64::from(step) / 22.0;
            let x = bbox.min_x + fraction * (bbox.max_x - bbox.min_x);
            let y = bbox.min_y + fraction * (bbox.max_y - bbox.min_y);
            for point in [
                (x, bbox.min_y),
                (x, bbox.max_y),
                (bbox.min_x, y),
                (bbox.max_x, y),
            ] {
                let (x, y) = proj.convert(point)?;
                let tolerance = 1e-6;
                assert!(
                    x >= transformed.min_x - tolerance
                        && x <= transformed.max_x + tolerance
                        && y >= transformed.min_y - tolerance
                        && y <= transformed.max_y + tolerance,
                    "boundary point {point:?} projects to ({x}, {y}) outside {transformed:?}"
                );
            }
        }
        assert!(
            (transformed.max_x - transformed.min_x - 5659.6402).abs() < 0.1,
            "Réunion footprint must span about 5660 m, not the SW/NE strip: {transformed:?}"
        );
        Ok(())
    }

    #[test]
    fn test_bounding_box_transform_curved_edge() -> Result<()> {
        // Lambert-93's southern edge reaches its minimum at the central meridian,
        // between the corners of this bbox (symmetric about longitude 3°).
        let bbox = BoundingBox::new(-5.0, 44.0, 11.0, 50.0);
        let transformed = bbox.transform(4326, 2154)?;
        let (_, edge_y) = GeoCore::transform_coords(4326, 2154, 3.0, bbox.min_y)?;
        let (_, corner_y) = GeoCore::transform_coords(4326, 2154, bbox.min_x, bbox.min_y)?;
        assert!(edge_y < corner_y - 1000.0, "edge must curve below corners");
        assert!(
            transformed.min_y <= edge_y + 1e-6,
            "curved edge minimum {edge_y} is outside {transformed:?}"
        );
        Ok(())
    }

    #[test]
    fn test_transform_coords() {
        // Test coordinate transformation (if proj data is available)
        // This test may fail if proj data is not installed
        if let Ok((x, y)) = GeoCore::transform_coords(4326, 2154, 2.0, 48.0) {
            assert!(x.is_finite());
            assert!(y.is_finite());
        }
    }
}
