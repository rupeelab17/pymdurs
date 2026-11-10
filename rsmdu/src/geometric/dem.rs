use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// DEM (Digital Elevation Model) structure
/// Following Python implementation from pymdu.geometric.Dem
/// Provides methods to collect and process DEM data from IGN API
pub struct Dem {
    /// IgnCollect instance for API requests
    ign_collect: IgnCollect,
    /// Output path for processed data
    output_path: PathBuf,
    /// Path to save the final DEM TIFF file
    path_save_tiff: PathBuf,
    /// Path to save the mask shapefile
    path_save_mask: PathBuf,
    /// Path to temporary TIFF file from IGN API
    path_temp_tiff: PathBuf,
    /// Path to save the clipped DEM TIFF file
    // path_save_tiff_clip: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the DEM area
    bbox: Option<BoundingBox>,
}

impl Dem {
    /// Create a new Dem instance
    /// Following Python: def __init__(self, output_path: str | None = None)
    pub fn new(output_path: Option<String>) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);

        let path_save_tiff = output_path_buf.join("DEM.tif");
        // let path_save_tiff_clip = output_path_buf.join("DEM_clip.tif");
        let path_save_mask = output_path_buf.join("mask.shp");
        let path_temp_tiff = get_temp_path().join("dem.tiff");

        // Remove existing files if they exist (following Python)
        // Python: if os.path.exists(self.path_temp_tiff): os.remove(self.path_temp_tiff)
        if path_temp_tiff.exists() {
            std::fs::remove_file(&path_temp_tiff).context(format!(
                "Failed to remove existing file: {:?}",
                path_temp_tiff
            ))?;
        }
        if path_save_tiff.exists() {
            std::fs::remove_file(&path_save_tiff).context(format!(
                "Failed to remove existing file: {:?}",
                path_save_tiff
            ))?;
        }
        /*if path_save_tiff_clip.exists() {
            std::fs::remove_file(&path_save_tiff_clip).context(format!(
                "Failed to remove existing file: {:?}",
                path_save_tiff_clip
            ))?;
        }*/

        Ok(Dem {
            ign_collect: IgnCollect::new()?,
            output_path: output_path_buf,
            path_save_tiff,
            path_save_mask,
            path_temp_tiff,
            //path_save_tiff_clip,
            geo_core: GeoCore::default(), // Default to EPSG:2154 (Lambert-93)
            bbox: None,
        })
    }

    /// Set bounding box
    /// Following Python: self.Bbox = [...]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
        self.ign_collect.set_bbox(min_x, min_y, max_x, max_y);
    }

    /// Set CRS
    /// Following Python: self._epsg (defaults to 2154)
    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core = GeoCore::new(epsg);
    }

    /// Run DEM processing
    /// Following Python: def run(self, shape: tuple = None)
    /// Downloads DEM from IGN API, reprojects it, and saves it
    pub fn run(mut self, shape: Option<(u32, u32)>) -> Result<Self> {
        self.run_internal(shape)?;
        Ok(self)
    }

    /// Internal run method that can be called mutably
    /// Used by Python bindings to avoid ownership issues
    pub fn run_internal(&mut self, shape: Option<(u32, u32)>) -> Result<()> {
        // Python: self.content = self.execute_ign(key="dem").content
        self.ign_collect.execute_ign("dem")?;

        // The DEM TIFF should have been saved to path_temp_tiff by execute_wms
        // Python: dataarray = rxr.open_rasterio(self.path_temp_tiff)
        if !self.path_temp_tiff.exists() {
            anyhow::bail!(
                "DEM file not found at {:?}. Make sure execute_ign('dem') was called successfully.",
                self.path_temp_tiff
            );
        }

        // Reproject and save
        // Python: self.dataarray = dataarray.rio.reproject(dst_crs=self._epsg, resolution=1, ...)
        self.reproject_and_save(shape)?;

        // Generate mask
        // Python: self.__generate_mask_and_adapt_dem()
        self.generate_mask_and_adapt_dem()?;

        Ok(())
    }

    /// Reproject raster and save to output file
    /// Following Python: dataarray.rio.reproject(...)
    /// NOTE: Full GDAL reprojection is complex - this is a placeholder
    /// TODO: Implement full raster reprojection using GDAL or dedicated raster crate
    /// Python: dataarray.rio.reproject(dst_crs=self._epsg, resolution=1, resampling=Resampling.nearest)
    ///         dataarray.rio.to_raster(..., compress="lzw", bigtiff="YES", ...)
    fn reproject_and_save(&self, _shape: Option<(u32, u32)>) -> Result<()> {
        // For now, copy the file as-is
        // Full reprojection would require:
        // 1. Reading the input GeoTIFF with geotiff or gdal
        // 2. Reprojecting to target CRS (EPSG:2154 by default) using proj
        // 3. Resampling to 1m resolution
        // 4. Saving with LZW compression using gdal

        // Create output directory if it doesn't exist
        if let Some(parent) = self.path_save_tiff.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }

        // Copy temp file to output (simplified - should reproject)
        std::fs::copy(&self.path_temp_tiff, &self.path_save_tiff).context(format!(
            "Failed to copy DEM from {:?} to {:?}",
            self.path_temp_tiff, self.path_save_tiff
        ))?;

        tracing::info!(
            "DEM saved to: {:?} (reprojection temporarily disabled)",
            self.path_save_tiff
        );
        tracing::info!(
            "  TODO: Implement full GDAL reprojection to EPSG:{}",
            self.geo_core.get_epsg()
        );
        tracing::info!(
            "  Python equivalent: dataarray.rio.reproject(dst_crs={}, resolution=1)",
            self.geo_core.get_epsg()
        );

        Ok(())
    }

    /// Generate mask shapefile and adapt DEM
    /// Following Python: def __generate_mask_and_adapt_dem(self)
    fn generate_mask_and_adapt_dem(&self) -> Result<()> {
        let bbox = self
            .bbox
            .context("Bounding box must be set before generating mask")?;

        // Python: gdf_project = gpd.GeoDataFrame(gpd.GeoSeries(box(...)), crs="epsg:4326")
        //         gdf_project = gdf_project.to_crs(epsg=2154)
        // Create bounding box polygon in EPSG:4326
        use geo::Rect;
        let rect_4326 = Rect::new(
            geo::coord! { x: bbox.min_x, y: bbox.min_y },
            geo::coord! { x: bbox.max_x, y: bbox.max_y },
        );

        // Transform to EPSG:2154 (Lambert-93)
        use geo::algorithm::map_coords::MapCoords;
        use proj::Proj;

        let proj_4326_to_2154 = Proj::new_known_crs("EPSG:4326", "EPSG:2154", None)
            .context("Failed to create projection from EPSG:4326 to EPSG:2154")?;

        // Transform the rectangle
        let rect_2154 = rect_4326.map_coords(|c| {
            let (x, y) = proj_4326_to_2154.convert((c.x, c.y)).unwrap_or((c.x, c.y));
            geo::coord! { x: x, y: y }
        });

        // Python: envelope_polygon = gdf_project.envelope.bounds
        //         Bbox = envelope_polygon.values[0]
        //         Bbox_final = box(Bbox[0], Bbox[1], Bbox[2], Bbox[3])
        let min_x = rect_2154.min().x;
        let min_y = rect_2154.min().y;
        let max_x = rect_2154.max().x;
        let max_y = rect_2154.max().y;

        // Python: gdf_Bbox_mask_2154.scale(xfact=0.85, yfact=0.85)
        // Scale the Bbox by 0.85
        let center_x = (min_x + max_x) / 2.0;
        let center_y = (min_y + max_y) / 2.0;
        let width = max_x - min_x;
        let height = max_y - min_y;

        let scaled_min_x = center_x - (width * 0.85 / 2.0);
        let scaled_min_y = center_y - (height * 0.85 / 2.0);
        let scaled_max_x = center_x + (width * 0.85 / 2.0);
        let scaled_max_y = center_y + (height * 0.85 / 2.0);

        // Create scaled rectangle
        // Python: bbox_final = box(bbox[0], bbox[1], bbox[2], bbox[3])
        //         gdf_bbox_mask_2154 = gpd.GeoDataFrame(gpd.GeoSeries(bbox_final), columns=["geometry"], crs="epsg:2154")
        let scaled_rect = Rect::new(
            geo::coord! { x: scaled_min_x, y: scaled_min_y },
            geo::coord! { x: scaled_max_x, y: scaled_max_y },
        );

        // Convert Rect to Polygon for shapefile export
        let polygon: geo::Polygon<f64> = scaled_rect.into();

        // Export to shapefile
        // Python: gdf_bbox_mask_2154.scale(xfact=0.85, yfact=0.85).to_file(self.path_save_mask, driver="ESRI Shapefile")
        #[cfg(not(feature = "wasm"))]
        {
            self.export_mask_to_shapefile(&polygon)?;
            tracing::info!("Mask shapefile saved to: {:?}", self.path_save_mask);
            //self.warp_and_clip_dem(&self.path_save_tiff, &self.path_save_tiff_clip)?;
            // println!("DEM warped and clipped to: {:?}", self.path_save_mask);
        }

        #[cfg(feature = "wasm")]
        {
            tracing::info!("Mask geometry created (shapefile export disabled in WASM mode)");
        }

        tracing::info!(
            "  Scaled Bbox (EPSG:2154): ({:.2}, {:.2}) to ({:.2}, {:.2})",
            scaled_min_x, scaled_min_y, scaled_max_x, scaled_max_y
        );
        tracing::info!(
            "  Original Bbox (EPSG:2154): ({:.2}, {:.2}) to ({:.2}, {:.2})",
            min_x, min_y, max_x, max_y
        );

        Ok(())
    }

    /// Export mask polygon to shapefile
    /// Following Python: gdf.to_file(self.path_save_mask, driver="ESRI Shapefile")
    /// Uses ogr2ogr command-line tool for reliable shapefile creation
    #[cfg(not(feature = "wasm"))]
    fn export_mask_to_shapefile(&self, polygon: &geo::Polygon<f64>) -> Result<()> {
        use std::io::Write;
        use std::process::Command;

        // Remove existing shapefile if it exists
        if self.path_save_mask.exists() {
            // Remove all shapefile components (.shp, .shx, .dbf, .prj)
            let base_path = self.path_save_mask.with_extension("");
            for ext in &[".shp", ".shx", ".dbf", ".prj"] {
                let file_path = base_path.with_extension(ext);
                if file_path.exists() {
                    let _ = std::fs::remove_file(&file_path);
                }
            }
        }

        // Create output directory if it doesn't exist
        if let Some(parent) = self.path_save_mask.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }

        let mut tmp = tempfile::Builder::new()
            .prefix("mask_")
            .suffix(".geojson")
            .tempfile()
            .context("Failed to create temporary GeoJSON file")?;

        // Convert polygon to GeoJSON
        use geojson::{Feature, GeoJson};
        let geometry: geojson::Geometry = polygon.into();

        let feature = Feature {
            bbox: None,
            geometry: Some(geometry),
            id: None,
            properties: None,
            foreign_members: None,
        };

        let geojson = GeoJson::FeatureCollection(geojson::FeatureCollection {
            bbox: None,
            features: vec![feature],
            foreign_members: None,
        });

        tmp.write_all(geojson.to_string().as_bytes())
            .context("Failed to write temporary GeoJSON file")?;
        tmp.flush()
            .context("Failed to flush temporary GeoJSON file")?;

        // Use ogr2ogr to convert GeoJSON to Shapefile
        // The polygon is already in EPSG:2154, so we specify the source CRS
        // Python: gdf_bbox_mask_2154 has crs="epsg:2154"
        let status = Command::new("ogr2ogr")
            .arg("-f")
            .arg("ESRI Shapefile")
            .arg("-s_srs")
            .arg("EPSG:2154") // Source CRS: polygon is already in EPSG:2154
            .arg("-t_srs")
            .arg("EPSG:2154") // Target CRS: keep EPSG:2154
            .arg(&self.path_save_mask)
            .arg(tmp.path())
            .status()
            .context(
                "Failed to execute ogr2ogr. Make sure GDAL is installed and ogr2ogr is in PATH",
            )?;

        if !status.success() {
            anyhow::bail!("ogr2ogr failed to convert GeoJSON to shapefile");
        }

        tracing::info!("Mask shapefile saved to: {:?}", self.path_save_mask);

        Ok(())
    }

    /// Warp and clip DEM using GDAL Warp
    /// Following Python: gdal.Warp(destNameOrDestDS='DEM_clip.tif', srcDSOrSrcDSTab='DEM.tif', options=warp_options)
    /// Uses gdalwarp command-line tool for reliable raster warping and clipping
    #[cfg(not(feature = "wasm"))]
    pub fn warp_and_clip_dem(&self, input_dem_path: &Path, output_clip_path: &Path) -> Result<()> {
        use std::process::Command;

        // Ensure mask shapefile exists
        if !self.path_save_mask.exists() {
            anyhow::bail!(
                "Mask shapefile not found at {:?}. Call generate_mask_and_adapt_dem() first.",
                self.path_save_mask
            );
        }

        // Ensure input DEM exists
        if !input_dem_path.exists() {
            anyhow::bail!("Input DEM not found at {:?}", input_dem_path);
        }

        // Create output directory if it doesn't exist
        if let Some(parent) = output_clip_path.parent() {
            std::fs::create_dir_all(parent)
                .context(format!("Failed to create output directory: {:?}", parent))?;
        }

        // Remove existing output file if it exists
        if output_clip_path.exists() {
            std::fs::remove_file(output_clip_path).context(format!(
                "Failed to remove existing file: {:?}",
                output_clip_path
            ))?;
        }

        // Build gdalwarp command with options equivalent to Python gdal.WarpOptions
        // Python options:
        //   format='GTiff'
        //   xRes=1, yRes=1
        //   outputType=gdalconst.GDT_Float32
        //   dstNodata=None
        //   dstSRS='EPSG:2154'
        //   cropToCutline=True
        //   cutlineDSName='mask.shp'
        //   cutlineLayer='mask'
        let status = Command::new("gdalwarp")
            .arg("-of")
            .arg("GTiff") // format='GTiff'
            .arg("-tr")
            .arg("1")
            .arg("1") // xRes=1, yRes=1
            .arg("-ot")
            .arg("Float32") // outputType=gdalconst.GDT_Float32
            .arg("-t_srs")
            .arg("EPSG:2154") // dstSRS='EPSG:2154'
            .arg("-crop_to_cutline") // cropToCutline=True
            .arg("-cutline")
            .arg(&self.path_save_mask) // cutlineDSName='mask.shp'
            .arg("-cl")
            .arg("mask") // cutlineLayer='mask'
            .arg("-co")
            .arg("COMPRESS=LZW") // Add compression like Python version
            .arg(input_dem_path)
            .arg(output_clip_path)
            .status()
            .context(
                "Failed to execute gdalwarp. Make sure GDAL is installed and gdalwarp is in PATH",
            )?;

        if !status.success() {
            anyhow::bail!("gdalwarp failed to warp and clip DEM");
        }

        tracing::info!("DEM warped and clipped to: {:?}", output_clip_path);

        Ok(())
    }

    /// Get the content from IGN API
    /// Following Python: def content(self): return self.content
    pub fn content(&self) -> Option<&Vec<u8>> {
        self.ign_collect.content.as_ref()
    }

    /// Get path to saved TIFF file
    pub fn get_path_save_tiff(&self) -> &Path {
        &self.path_save_tiff
    }

    /// Get path to mask shapefile
    pub fn get_path_save_mask(&self) -> &Path {
        &self.path_save_mask
    }

    /// Get output path
    /// Following Python: self.output_path
    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dem_new() {
        let dem = Dem::new(None).unwrap();
        assert!(dem.path_save_tiff.to_string_lossy().contains("DEM.tif"));
    }

    #[test]
    fn test_dem_set_bbox() {
        let mut dem = Dem::new(None).unwrap();
        dem.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);
        assert!(dem.bbox.is_some());
    }

    #[test]
    fn test_dem_set_crs() {
        let mut dem = Dem::new(None).unwrap();
        dem.set_crs(2154);
        assert_eq!(dem.geo_core.get_epsg(), 2154);
    }
}
