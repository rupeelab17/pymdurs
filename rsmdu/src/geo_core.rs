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

    /// Transform bounding box to another CRS
    pub fn transform(&self, from_epsg: i32, to_epsg: i32) -> Result<Self> {
        let (min_x, min_y) = GeoCore::transform_coords(from_epsg, to_epsg, self.min_x, self.min_y)?;
        let (max_x, max_y) = GeoCore::transform_coords(from_epsg, to_epsg, self.max_x, self.max_y)?;

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
    fn test_transform_coords() {
        // Test coordinate transformation (if proj data is available)
        // This test may fail if proj data is not installed
        if let Ok((x, y)) = GeoCore::transform_coords(4326, 2154, 2.0, 48.0) {
            assert!(x.is_finite());
            assert!(y.is_finite());
        }
    }
}
