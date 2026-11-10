//! Building collection processing (run, bbox, heights pipeline).

use anyhow::{Context, Result};
use geo::algorithm::map_coords::MapCoordsInPlace;
use geo::{Area, Centroid, Coord};
use proj::Proj;

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::BoundingBox;

use super::types::BuildingCollection;

impl BuildingCollection {
    /// Run processing: load data, process heights, return self
    /// Following Python: def run(self)
    /// Loads buildings from IGN API or shapefile, processes heights, and returns self
    pub fn run(mut self) -> Result<Self> {
        self.run_internal()?;
        Ok(self)
    }

    /// Internal run method that can be called mutably
    /// Used by Python bindings to avoid ownership issues
    pub fn run_internal(&mut self) -> Result<()> {
        if self.filepath_shp.is_none() {
            // Load from IGN API
            let mut ign_collect = self
                .ign_collect
                .take()
                .unwrap_or_else(|| IgnCollect::new().expect("Failed to create IgnCollect"));

            // Execute IGN API request
            ign_collect.execute_ign("buildings")?;

            // Get GeoJSON content as bytes
            let geojson_bytes = ign_collect
                .content
                .as_ref()
                .context("No content available from IGN API")?;

            // Load from GeoJSON (coords stay EPSG:4326 until reproject_from_wgs84)
            let output_path = self.geo_core.get_output_path().cloned();
            let mut new_collection = Self::from_geojson(
                geojson_bytes,
                output_path,
                self.default_storey_height,
                Some(self.geo_core.get_epsg()),
            )?;

            // Preserve GeoCore state (Bbox, output_path, etc.)
            new_collection.geo_core = self.geo_core.clone();
            *self = new_collection;
        } else {
            // NOTE: Shapefile loading is temporarily disabled due to GDAL issues
            anyhow::bail!("Shapefile loading is temporarily disabled. Use from_geojson() or from_ign_api() instead.");
        }

        // IGN WFS returns WGS84; match Python gdf.to_crs(self._epsg)
        self.reproject_from_wgs84()?;
        self.process_heights();

        Ok(())
    }

    /// Reproject footprints from EPSG:4326 to `geo_core.epsg`, then refresh area/centroid.
    /// No-op when target CRS is already 4326.
    pub fn reproject_from_wgs84(&mut self) -> Result<()> {
        let to_epsg = self.geo_core.get_epsg();
        if to_epsg == 4326 {
            return Ok(());
        }

        let to_crs = format!("EPSG:{to_epsg}");
        let proj = Proj::new_known_crs("EPSG:4326", &to_crs, None)
            .context("Failed to create Proj 4326 → target CRS")?;

        for building in &mut self.buildings {
            building
                .footprint
                .try_map_coords_in_place(|Coord { x, y }| -> Result<Coord> {
                    let (nx, ny) = proj
                        .convert((x, y))
                        .with_context(|| format!("reproject building vertex ({x}, {y})"))?;
                    Ok(Coord { x: nx, y: ny })
                })?;
            building.area = building.footprint.unsigned_area();
            building.centroid = building
                .footprint
                .centroid()
                .unwrap_or_else(|| geo::Point::new(0.0, 0.0));
        }
        Ok(())
    }

    /// Set bounding box for IGN API requests
    /// Following Python: self.set_bbox = [...]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Result<()> {
        let bbox = BoundingBox::new(min_x, min_y, max_x, max_y);
        self.geo_core.set_bbox(Some(bbox));
        if let Some(ref mut ign_collect) = self.ign_collect {
            ign_collect.set_bbox(min_x, min_y, max_x, max_y);
        } else {
            // Create IgnCollect if needed
            let mut ign_collect = IgnCollect::new()?;
            ign_collect.set_bbox(min_x, min_y, max_x, max_y);
            self.ign_collect = Some(ign_collect);
        }
        Ok(())
    }
}
