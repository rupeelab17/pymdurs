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

    /// Build an axis-aligned mask in the configured CRS, inset by 15%.
    fn mask_polygon(&self) -> Result<geo::Polygon<f64>> {
        let bbox = self
            .bbox
            .context("Bounding box must be set before generating mask")?;

        // Rect::map_coords transforms only min/max, losing the other corners and
        // curved edges. Bound the full footprint in the target CRS before insetting.
        let bounds = bbox
            .transform(4326, self.geo_core.epsg)
            .context("Failed to transform DEM mask bounds")?;
        let BoundingBox {
            min_x,
            min_y,
            max_x,
            max_y,
        } = bounds;
        let center_x = (min_x + max_x) / 2.0;
        let center_y = (min_y + max_y) / 2.0;
        let width = max_x - min_x;
        let height = max_y - min_y;

        let scaled_min_x = center_x - (width * 0.85 / 2.0);
        let scaled_min_y = center_y - (height * 0.85 / 2.0);
        let scaled_max_x = center_x + (width * 0.85 / 2.0);
        let scaled_max_y = center_y + (height * 0.85 / 2.0);

        let scaled_rect = geo::Rect::new(
            geo::coord! { x: scaled_min_x, y: scaled_min_y },
            geo::coord! { x: scaled_max_x, y: scaled_max_y },
        );

        Ok(scaled_rect.into())
    }

    /// Generate mask shapefile and adapt DEM
    /// Following Python: def __generate_mask_and_adapt_dem(self)
    fn generate_mask_and_adapt_dem(&self) -> Result<()> {
        let polygon = self.mask_polygon()?;

        // Coordinates and shapefile metadata must use the same configured CRS.
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

        tracing::info!(epsg = self.geo_core.epsg, "DEM mask uses configured CRS");

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

        // Assign the polygon's CRS without reprojecting its native coordinates.
        let status = Command::new("ogr2ogr")
            .arg("-f")
            .arg("ESRI Shapefile")
            .arg("-a_srs")
            .arg(format!("EPSG:{}", self.geo_core.epsg))
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
        //   dstSRS=self.geo_core.epsg
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
            .arg(format!("EPSG:{}", self.geo_core.epsg))
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
    use geo::BoundingRect;

    // Avoid Dem::new's cleanup of the shared temporary DEM in offline tests.
    fn reunion_dem(output_path: &Path) -> Result<Dem> {
        Ok(Dem {
            ign_collect: IgnCollect::new()?,
            output_path: output_path.to_path_buf(),
            path_save_tiff: output_path.join("DEM.tif"),
            path_save_mask: output_path.join("mask.shp"),
            path_temp_tiff: output_path.join("dem.tiff"),
            geo_core: GeoCore::new(2975),
            bbox: Some(BoundingBox::new(55.51, -21.285, 55.53, -21.265)),
        })
    }

    #[test]
    fn test_dem_mask_reunion_native_axis_aligned() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let dem = reunion_dem(dir.path())?;
        let polygon = dem.mask_polygon()?;
        let bounds = polygon.bounding_rect().context("Missing mask bounds")?;
        let projected = dem
            .bbox
            .context("Missing test bbox")?
            .transform(4326, 2975)?;

        assert!((bounds.width() - 0.85 * (projected.max_x - projected.min_x)).abs() < 1e-6);
        assert!((bounds.height() - 0.85 * (projected.max_y - projected.min_y)).abs() < 1e-6);
        assert!((bounds.center().x - (projected.min_x + projected.max_x) / 2.0).abs() < 1e-6);
        assert!((bounds.center().y - (projected.min_y + projected.max_y) / 2.0).abs() < 1e-6);
        assert!(
            (300_000.0..400_000.0).contains(&bounds.min().x),
            "{bounds:?}"
        );
        assert!(
            (7_600_000.0..7_700_000.0).contains(&bounds.min().y),
            "{bounds:?}"
        );
        assert!(
            bounds.width() > 1700.0 && bounds.height() > 1800.0,
            "{bounds:?}"
        );
        for edge in polygon.exterior().0.windows(2) {
            assert!(
                edge[0].x == edge[1].x || edge[0].y == edge[1].y,
                "diagonal edge: {edge:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn test_dem_mask_lambert93_uses_full_footprint() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut dem = reunion_dem(dir.path())?;
        dem.set_crs(2154);
        let bounds = dem
            .mask_polygon()?
            .bounding_rect()
            .context("Missing mask bounds")?;
        assert!(
            (bounds.width() - 0.85 * 5659.6402).abs() < 0.1,
            "{bounds:?}"
        );
        Ok(())
    }

    #[test]
    fn test_dem_mask_transform_errors_propagate() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut dem = reunion_dem(dir.path())?;
        dem.set_crs(-1);
        assert!(
            dem.mask_polygon().is_err(),
            "invalid CRS must not fall back to degrees"
        );
        dem.set_crs(2975);
        dem.set_bbox(55.51, 95.0, 55.53, 96.0);
        assert!(
            dem.mask_polygon().is_err(),
            "invalid latitude must not fall back to degrees"
        );
        Ok(())
    }

    #[cfg(all(not(feature = "wasm"), feature = "gdal"))]
    #[test]
    fn test_dem_mask_shapefile_native_geometry_and_prj() -> Result<()> {
        use gdal::vector::LayerAccess;
        use std::process::Command;

        match Command::new("ogr2ogr").arg("--version").output() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("Skipping shapefile regression: ogr2ogr is not installed");
                return Ok(());
            }
            result => anyhow::ensure!(result?.status.success(), "ogr2ogr --version failed"),
        }
        let dir = tempfile::tempdir()?;
        let dem = reunion_dem(dir.path())?;
        dem.generate_mask_and_adapt_dem()?;
        assert!(dem.path_save_mask.with_extension("prj").is_file());
        let dataset = gdal::Dataset::open(&dem.path_save_mask)?;
        let mut layer = dataset.layer(0)?;
        let srs = layer.spatial_ref().context("Missing shapefile CRS")?;
        assert_eq!(srs, gdal::spatial_ref::SpatialRef::from_epsg(2975)?);
        let feature = layer.features().next().context("Missing mask feature")?;
        let geometry = feature.geometry().context("Missing mask geometry")?;
        let ring = geometry.get_geometry(0);
        let expected = dem.mask_polygon()?;
        assert_eq!(ring.point_count(), expected.exterior().0.len());
        for index in 0..ring.point_count() {
            let (x, y, _) = ring.get_point(index as i32);
            assert!(
                expected
                    .exterior()
                    .0
                    .iter()
                    .any(|c| (c.x - x).abs() < 1e-6 && (c.y - y).abs() < 1e-6),
                "shapefile coordinate ({x}, {y}) differs from native mask"
            );
            if index > 0 {
                let (previous_x, previous_y, _) = ring.get_point(index as i32 - 1);
                assert!(
                    x == previous_x || y == previous_y,
                    "diagonal shapefile edge"
                );
            }
        }
        Ok(())
    }

    #[cfg(all(not(feature = "wasm"), feature = "gdal"))]
    #[test]
    fn test_dem_warp_and_clip_native_crs_offline() -> Result<()> {
        use std::process::Command;

        for tool in ["ogr2ogr", "gdalwarp"] {
            match Command::new(tool).arg("--version").output() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    eprintln!("Skipping warp regression: {tool} is not installed");
                    return Ok(());
                }
                result => anyhow::ensure!(result?.status.success(), "{tool} --version failed"),
            }
        }
        let dir = tempfile::tempdir()?;
        let dem = reunion_dem(dir.path())?;
        dem.generate_mask_and_adapt_dem()?;
        let bounds = dem
            .mask_polygon()?
            .bounding_rect()
            .context("Missing mask bounds")?;
        let srs = gdal::spatial_ref::SpatialRef::from_epsg(2975)?;
        let driver = gdal::DriverManager::get_driver_by_name("GTiff")?;
        let mut source = driver.create_with_band_type::<f32, _>(
            &dem.path_save_tiff,
            bounds.width().ceil() as usize + 200,
            bounds.height().ceil() as usize + 200,
            1,
        )?;
        source.set_spatial_ref(&srs)?;
        source.set_geo_transform(&[
            bounds.min().x - 100.0,
            1.0,
            0.0,
            bounds.max().y + 100.0,
            0.0,
            -1.0,
        ])?;
        source.rasterband(1)?.fill(42.0, None)?;
        source.close()?;

        let output_path = dir.path().join("DEM_clip.tif");
        dem.warp_and_clip_dem(&dem.path_save_tiff, &output_path)?;
        let clipped = gdal::Dataset::open(output_path)?;
        assert_eq!(clipped.spatial_ref()?, srs);
        let transform = clipped.geo_transform()?;
        let (width, height) = clipped.raster_size();
        assert_eq!((transform[1], transform[5]), (1.0, -1.0));
        assert!((transform[0] - bounds.min().x).abs() <= 1.0);
        assert!((transform[3] - bounds.max().y).abs() <= 1.0);
        assert!((width as f64 - bounds.width()).abs() <= 2.0);
        assert!((height as f64 - bounds.height()).abs() <= 2.0);
        let pixels = clipped.rasterband(1)?.read_band_as::<f32>()?;

        // GDAL rounds the crop extent to whole pixels; only the outermost
        // row/column can lie outside the fractional cutline boundary.
        assert!(
            pixels
                .data()
                .chunks_exact(width)
                .skip(1)
                .take(height - 2)
                .all(|row| row[1..width - 1].iter().all(|&value| value == 42.0)),
            "native rectangular cutline must not introduce diagonal nodata"
        );
        Ok(())
    }

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
