//! Building collection processing (run, bbox, heights pipeline).

use anyhow::{Context, Result};

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
        // Python: if not self.filepath_shp:
        //         self.execute_ign(key="buildings")
        //         file = self.content if isinstance(self.content, io.BytesIO) else io.BytesIO(self.content)
        //         gdf = gpd.read_file(file, driver="GeoJSON")
        // else:
        //     gdf = gpd.read_file(self.filepath_shp, driver="ESRI Shapefile")

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

            // Load from GeoJSON
            // Get output_path from GeoCore (Python: self.output_path)
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
            // Load from shapefile
            // Python: gdf = gpd.read_file(self.filepath_shp, driver="ESRI Shapefile")
            // NOTE: Shapefile loading is temporarily disabled due to GDAL issues
            anyhow::bail!("Shapefile loading is temporarily disabled. Use from_geojson() or from_ign_api() instead.");
        }

        // Python: if self.set_crs:
        //         gdf = gdf.set_crs(crs=self.set_crs, inplace=True, allow_override=True)
        // else:
        //     gdf = gdf.to_crs(self._epsg)
        // CRS is already handled in from_geojson()

        // Process heights (following Python logic)
        // Python: gdf["noHauteur"] = gdf["hauteur"].isnull()
        //         ... (calculations) ...
        //         gdf.dropna(subset=["hauteur"], inplace=True)
        self.process_heights();

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
