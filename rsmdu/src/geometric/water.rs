use anyhow::{Context, Result};
use gdal::Dataset;
use geojson::GeoJson;
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// Water structure
/// Following Python implementation from pymdu.geometric.Water
/// Provides methods to collect and process Water (plan d'eau) data from IGN API or shapefile
pub struct Water {
    /// Optional shapefile path (Python: filepath_shp)
    filepath_shp: Option<String>,
    /// IgnCollect instance for API requests
    ign_collect: Option<IgnCollect>,
    /// Output path for processed data
    output_path: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the water area
    bbox: Option<BoundingBox>,
    /// Parsed GeoJSON content
    geojson: Option<GeoJson>,
    /// CRS to set if provided (Python: set_crs)
    set_crs: Option<i32>,
}

impl Water {
    /// Create a new Water instance
    /// Following Python: def __init__(self, filepath_shp=None, output_path=None, set_crs=None)
    pub fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        set_crs: Option<i32>,
    ) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);

        let mut geo_core = if let Some(epsg) = set_crs {
            GeoCore::new(epsg)
        } else {
            GeoCore::default() // Default to EPSG:2154 (Lambert-93)
        };

        // Set output_path in GeoCore
        geo_core.set_output_path(Some(output_path_buf.to_string_lossy().to_string()));

        let mut water = Water {
            filepath_shp,
            ign_collect: None,
            output_path: output_path_buf,
            geo_core,
            bbox: None,
            geojson: None,
            set_crs,
        };

        // Initialize IgnCollect if no shapefile provided (will be used for IGN API)
        // Python: if not self.filepath_shp: osm = OsmCollect(key='"natural"="water"')
        // For now, we use IgnCollect with key "water" (BDTOPO_V3:plan_d_eau)
        // TODO: Implement OsmCollect for OSM data if needed
        if water.filepath_shp.is_none() {
            water.ign_collect = Some(IgnCollect::new()?);
        }

        Ok(water)
    }

    /// Set bounding box
    /// Following Python: water.bbox = [min_x, min_y, max_x, max_y]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        if let Some(ref mut ign_collect) = self.ign_collect {
            ign_collect.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        }
    }

    /// Set CRS
    /// Following Python: water.set_crs = epsg
    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core.set_epsg(epsg);
        if let Some(ref mut ign_collect) = self.ign_collect {
            ign_collect.geo_core.set_epsg(epsg);
        }
        self.set_crs = Some(epsg);
    }

    /// Run water processing: download from IGN API or load from shapefile, parse GeoJSON
    /// Following Python: def run(self) -> self
    pub fn run(mut self) -> Result<Self> {
        self.run_internal()?;
        Ok(self)
    }

    /// Internal run method that can be called mutably
    /// Used by Python bindings to avoid ownership issues
    pub fn run_internal(&mut self) -> Result<()> {
        // Python: if not self.filepath_shp:
        //         osm = OsmCollect(key='"natural"="water"')
        //         self.gdf = osm.run().to_gdf()
        // else:
        //     self.gdf = gpd.read_file(self.filepath_shp, driver="ESRI Shapefile")
        if self.filepath_shp.is_none() {
            // Load from IGN API using IgnCollect with key "water"
            // Python uses OsmCollect, but we use IgnCollect for IGN BDTOPO data
            // TODO: Implement OsmCollect if OSM data is specifically needed
            let mut ign_collect = self
                .ign_collect
                .take()
                .context("IgnCollect not initialized")?;

            // Execute IGN API request for water
            ign_collect
                .execute_ign("water")
                .context("Failed to execute IGN request for water")?;

            // Get content from IgnCollect
            let content = ign_collect
                .content
                .as_ref()
                .context("No content received from IGN API")?;

            // Parse GeoJSON following Python: gpd.read_file(file, driver="GeoJSON")
            let geojson_str = String::from_utf8_lossy(content);
            let geojson: GeoJson = geojson_str
                .parse()
                .context("Failed to parse GeoJSON from IGN API response")?;

            tracing::info!("GeoJSON: {:?}", geojson);

            self.geojson = Some(geojson);
        } else {
            // Load from shapefile
            // Python: self.gdf = gpd.read_file(self.filepath_shp, driver="ESRI Shapefile")
            self.load_from_shapefile()?;
        }

        // Python: if self.set_crs:
        //         self.gdf = self.gdf.set_crs(crs=self.set_crs, inplace=True, allow_override=True)
        // else:
        //     self.gdf = self.gdf.to_crs(epsg=self._epsg)
        // Note: CRS transformation would require GDAL reprojection
        // For now, we store the GeoJSON as-is
        // TODO: Implement CRS transformation using GDAL or proj crate

        Ok(())
    }

    /// Load water data from shapefile
    /// Following Python: gpd.read_file(self.filepath_shp, driver="ESRI Shapefile")
    fn load_from_shapefile(&mut self) -> Result<()> {
        // Copy values to avoid borrow checker issues
        let filepath = self
            .filepath_shp
            .as_ref()
            .context("No shapefile path provided")?
            .clone();
        let epsg_to_set = self.set_crs;

        // Handle CRS before opening dataset
        // If set_crs is provided, use it; otherwise keep the default
        // Note: Getting SRS from layer requires additional GDAL API calls
        // For now, we'll use set_crs if provided, or keep the default
        if let Some(epsg) = epsg_to_set {
            self.set_crs(epsg);
        }
        // TODO: Implement SRS detection from shapefile if set_crs is not provided

        // Open shapefile using GDAL (we don't actually need to use it since we use ogr2ogr)
        let _dataset =
            Dataset::open(&filepath).context(format!("Failed to open shapefile: {}", filepath))?;

        // Use ogr2ogr command-line tool for reliable shapefile to GeoJSON conversion
        // This is more reliable than using the GDAL Rust bindings directly
        // which have complex API requirements for vector dataset creation
        // Following the approach from lcz.rs
        use std::process::Command;

        // GeoJSON driver refuses overwrite: reserve a path then delete the empty tempfile.
        let tmp = tempfile::Builder::new()
            .prefix("water_")
            .suffix(".geojson")
            .tempfile()
            .context("Failed to create temporary GeoJSON file")?;
        let temp_geojson = tmp.path().to_path_buf();
        drop(tmp);

        let status = Command::new("ogr2ogr")
            .arg("-f")
            .arg("GeoJSON")
            .arg(&temp_geojson)
            .arg(&filepath)
            .status()
            .context(
                "Failed to execute ogr2ogr. Make sure GDAL is installed and ogr2ogr is in PATH",
            )?;

        if !status.success() {
            let _ = std::fs::remove_file(&temp_geojson);
            anyhow::bail!("ogr2ogr failed to convert shapefile to GeoJSON");
        }

        // Read the GeoJSON file we just created
        let geojson_bytes =
            std::fs::read(&temp_geojson).context("Failed to read temporary GeoJSON file")?;
        let _ = std::fs::remove_file(&temp_geojson);

        // Parse GeoJSON
        let geojson_str = String::from_utf8_lossy(&geojson_bytes);
        let geojson: GeoJson = geojson_str
            .parse()
            .context("Failed to parse GeoJSON from shapefile")?;

        self.geojson = Some(geojson);

        Ok(())
    }

    /// Get the GeoJSON (equivalent to to_gdf() in Python)
    /// Following Python: def to_gdf(self) -> gpd.GeoDataFrame
    pub fn get_geojson(&self) -> Option<&GeoJson> {
        self.geojson.as_ref()
    }

    /// Save to GeoJSON file
    /// Following Python: def to_geojson(self, name: str = "water")
    /// Note: GeoJSON export requires GDAL and is complex
    /// For now, we save as GeoJSON - full GeoJSON export would require GDAL layer operations
    /// TODO: Implement full GeoJSON export using GDAL
    pub fn to_geojson(&self, name: Option<&str>) -> Result<()> {
        // Python: self.gdf.to_file(f"{os.path.join(self.output_path, name)}.GeoJSON", driver="GeoJSON")
        // For now, save as GeoJSON as a workaround
        // Full GeoJSON export would require:
        // 1. Converting GeoJSON to GDAL Dataset
        // 2. Reprojecting to target CRS if needed
        // 3. Creating GeoJSON file with GDAL driver
        // 4. Copying layers and features

        let geojson = self
            .geojson
            .as_ref()
            .context("No GeoJSON data available. Call run() first.")?;

        let name = name.unwrap_or("water");

        // Save as GeoJSON for now (GeoJSON export is complex with GDAL Rust bindings)
        let output_file = self.output_path.join(format!("{}.geojson", name));
        let geojson_str = geojson.to_string();
        std::fs::write(&output_file, geojson_str)
            .context(format!("Failed to write GeoJSON file: {:?}", output_file))?;

        tracing::info!(
            "Water saved to: {:?} (as GeoJSON - GeoJSON export temporarily disabled)",
            output_file
        );

        Ok(())
    }

    /// Get output path
    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }
}
