//! Vegetation types and public API.

use anyhow::{Context, Result};
use geojson::GeoJson;
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// Vegetation structure
/// Following Python implementation from pymdu.geometric.Vegetation
/// Provides methods to collect and process Vegetation data from IGN API (NDVI) or shapefile
pub struct Vegetation {
    /// Optional shapefile path (Python: filepath_shp)
    pub(crate) filepath_shp: Option<String>,
    /// IgnCollect instance for API requests
    pub(crate) ign_collect: Option<IgnCollect>,
    /// Output path for processed data
    pub(crate) output_path: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the vegetation area
    pub(crate) bbox: Option<BoundingBox>,
    /// Parsed GeoJSON content
    pub(crate) geojson: Option<GeoJson>,
    /// CRS to set if provided (Python: set_crs)
    pub(crate) set_crs: Option<i32>,
    /// Minimum area filter (Python: min_area)
    pub(crate) min_area: f64,
    /// Write file flag (Python: write_file)
    #[allow(dead_code)]
    pub(crate) write_file: bool,
    /// Path to temporary IRC image
    pub(crate) img_tiff_path: PathBuf,
    /// Path to NDVI shapefile
    pub(crate) ndvi_shp_path: PathBuf,
    /// Path to NDVI GeoTIFF
    pub(crate) ndvi_tif_path: PathBuf,
}

impl Vegetation {
    /// Create a new Vegetation instance
    /// Following Python: def __init__(self, filepath_shp=None, output_path=None, set_crs=None, write_file=False, min_area=0)
    pub fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        set_crs: Option<i32>,
        write_file: bool,
        min_area: f64,
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

        // IgnCollect saves IRC image as {get_temp_path()}/irc.tiff
        let scratch = get_temp_path();
        let img_tiff_path = scratch.join("irc.tiff");
        let ndvi_shp_path = scratch.join("ndvi.shp");
        let ndvi_tif_path = scratch.join("ndvi.tif");

        let mut vegetation = Vegetation {
            filepath_shp,
            ign_collect: None,
            output_path: output_path_buf,
            geo_core,
            bbox: None,
            geojson: None,
            set_crs,
            min_area,
            write_file,
            img_tiff_path,
            ndvi_shp_path,
            ndvi_tif_path,
        };

        // Initialize IgnCollect if no shapefile provided (will be used for IGN API)
        if vegetation.filepath_shp.is_none() {
            vegetation.ign_collect = Some(IgnCollect::new()?);
        }

        Ok(vegetation)
    }

    /// Set bounding box
    /// Following Python: vegetation.bbox = [min_x, min_y, max_x, max_y]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        if let Some(ref mut ign_collect) = self.ign_collect {
            ign_collect.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        }
    }

    /// Set CRS
    /// Following Python: vegetation.set_crs = epsg
    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core.set_epsg(epsg);
        if let Some(ref mut ign_collect) = self.ign_collect {
            ign_collect.geo_core.set_epsg(epsg);
        }
        self.set_crs = Some(epsg);
    }

    /// Run vegetation processing: calculate NDVI from IRC or load from shapefile
    /// Following Python: def run(self) -> self
    pub fn run(mut self) -> Result<Self> {
        self.run_internal()?;
        Ok(self)
    }

    /// Internal run method that can be called mutably
    /// Used by Python bindings to avoid ownership issues
    pub fn run_internal(&mut self) -> Result<()> {
        // Python: if not self.filepath_shp:
        //         self.execute_ign(key="irc")
        //         ... (NDVI calculation and polygonization)
        // else:
        //     self.gdf = gpd.read_file(self.filepath_shp, driver="ESRI Shapefile")
        if self.filepath_shp.is_none() {
            // Load from IGN API and calculate NDVI
            self.calculate_ndvi_from_irc()?;
        } else {
            // Load from shapefile
            self.load_from_shapefile()?;
        }

        // Python: if self.set_crs:
        //         self.gdf = self.gdf.set_crs(crs=self.set_crs, inplace=True, allow_override=True)
        // else:
        //     self.gdf.crs = self._epsg
        // Note: CRS transformation would require GDAL reprojection
        // For now, we store the GeoJSON as-is
        // TODO: Implement CRS transformation using GDAL or proj crate

        Ok(())
    }

    /// Get the GeoJSON (equivalent to to_gdf() in Python)
    /// Following Python: def to_gdf(self) -> gpd.GeoDataFrame
    pub fn get_geojson(&self) -> Option<&GeoJson> {
        self.geojson.as_ref()
    }

    /// Save to GeoJSON file
    /// Following Python: def to_geojson(self, name: str = "vegetation")
    /// Note: GeoJSON export requires GDAL and is complex
    /// For now, we save as GeoJSON - full GeoJSON export would require GDAL layer operations
    /// TODO: Implement full GeoJSON export using GDAL
    pub fn to_geojson(&self, name: Option<&str>) -> Result<()> {
        let geojson = self
            .geojson
            .as_ref()
            .context("No GeoJSON data available. Call run() first.")?;

        let name = name.unwrap_or("vegetation");

        // Save as GeoJSON for now (GeoJSON export is complex with GDAL Rust bindings)
        let output_file = self.output_path.join(format!("{}.geojson", name));
        let geojson_str = geojson.to_string();
        std::fs::write(&output_file, geojson_str)
            .context(format!("Failed to write GeoJSON file: {:?}", output_file))?;

        tracing::info!(
            "Vegetation saved to: {:?} (as GeoJSON - GeoJSON export temporarily disabled)",
            output_file
        );

        Ok(())
    }

    /// Get output path
    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }

    /// Get minimum area filter
    pub fn get_min_area(&self) -> f64 {
        self.min_area
    }
}
