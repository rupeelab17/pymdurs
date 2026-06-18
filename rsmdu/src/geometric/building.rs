use anyhow::{Context, Result};
#[cfg(not(feature = "wasm"))]
#[allow(unused_imports)]
use gdal::vector::{Feature as GdalFeature, LayerAccess};
#[cfg(not(feature = "wasm"))]
#[allow(unused_imports)]
use gdal::{Dataset, DriverManager};
use geo::{Area, Centroid, Polygon};
use geojson::{Feature as GeoJsonFeature, GeoJson, Geometry};
// GeoPolars integration - geometry column is added as WKB bytes
// TODO: Use GeoDataFrame when GeoPolars API is stable
// // GeoPolars integration - geometry column is added as WKB bytes
// TODO: Use GeoDataFrame when GeoPolars API is stable
// use geopolars::geodataframe::GeoDataFrame;
#[cfg(not(feature = "wasm"))]
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[cfg(not(feature = "wasm"))]
use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::{BoundingBox, GeoCore};

/// Building structure representing a single building with its geometric and metadata properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Building {
    /// Building footprint as a polygon
    pub footprint: Polygon<f64>,
    /// Building height in meters
    pub height: Option<f64>,
    /// Building area in square meters
    pub area: f64,
    /// Building centroid
    pub centroid: geo::Point<f64>,
    /// Number of storeys (if available)
    pub nombre_d_etages: Option<f64>,
    /// Alternative height field (HAUTEUR_2)
    pub hauteur_2: Option<f64>,
    /// Flag indicating if height is null
    pub no_hauteur: bool,
    /// Additional metadata as key-value pairs
    pub metadata: HashMap<String, String>,
}

impl Building {
    /// Create a new Building from a polygon
    pub fn new(footprint: Polygon<f64>) -> Self {
        let area = footprint.unsigned_area();
        let centroid = footprint
            .centroid()
            .unwrap_or_else(|| geo::Point::new(0.0, 0.0));

        Building {
            footprint,
            height: None,
            area,
            centroid,
            nombre_d_etages: None,
            hauteur_2: None,
            no_hauteur: true,
            metadata: HashMap::new(),
        }
    }

    /// Create a Building with height
    pub fn with_height(footprint: Polygon<f64>, height: f64) -> Self {
        let mut building = Self::new(footprint);
        building.height = Some(height);
        building.no_hauteur = false;
        building
    }

    /// Set height
    pub fn set_height(&mut self, height: f64) {
        self.height = Some(height);
        self.no_hauteur = false;
    }

    /// Set number of storeys
    pub fn set_nombre_d_etages(&mut self, etages: f64) {
        self.nombre_d_etages = Some(etages);
    }

    /// Set alternative height
    pub fn set_hauteur_2(&mut self, hauteur: f64) {
        self.hauteur_2 = Some(hauteur);
    }

    /// Get height, calculating from storeys if needed
    pub fn get_height(&self, default_storey_height: f64) -> f64 {
        if let Some(h) = self.height {
            return h;
        }

        // Try to calculate from storeys
        if let Some(etages) = self.nombre_d_etages {
            return etages * default_storey_height;
        }

        // Try alternative height
        if let Some(h2) = self.hauteur_2 {
            return h2;
        }

        // Return 0 if no height available
        0.0
    }

    /// Calculate weighted area * height for mean height calculation
    /// Python: gdf["areaHauteur"] = gdf.apply(lambda x: x["area"] * x["hauteur"], axis=1)
    /// Only used for buildings that already have a height (not null)
    pub fn area_height_product(&self) -> Option<f64> {
        self.height.map(|h| self.area * h)
    }
}

/// Collection of buildings with processing capabilities
/// Following Python: class Building(IgnCollect)
/// In Python, Building inherits from IgnCollect, which inherits from GeoCore
/// In Rust, we use composition: BuildingCollection contains GeoCore and IgnCollect
pub struct BuildingCollection {
    pub buildings: Vec<Building>,
    pub default_storey_height: f64,
    /// GeoCore instance (Python: Building inherits from GeoCore via IgnCollect)
    /// Contains: epsg, Bbox, output_path, output_path_shp, filename_shp
    pub geo_core: GeoCore,
    /// Optional shapefile path (Python: filepath_shp)
    pub filepath_shp: Option<String>,
    /// IgnCollect instance for API requests (Python: Building inherits from IgnCollect)
    #[cfg(not(feature = "wasm"))]
    ign_collect: Option<IgnCollect>,
}

impl BuildingCollection {
    /// Create a new BuildingCollection
    /// Following Python: def __init__(self, filepath_shp=None, output_path=None, defaultStoreyHeight=3.0, set_crs=None)
    pub fn new(
        filepath_shp: Option<String>,
        output_path: Option<String>,
        default_storey_height: f64,
        set_crs: Option<i32>,
    ) -> Result<Self> {
        use crate::collect::global_variables::TEMP_PATH;

        let output_path_str = output_path.unwrap_or_else(|| TEMP_PATH.to_string());

        let mut geo_core = if let Some(epsg) = set_crs {
            GeoCore::new(epsg)
        } else {
            GeoCore::default() // Default to EPSG:2154
        };

        // Set output_path in GeoCore (Python: self.output_path = output_path)
        geo_core.set_output_path(Some(output_path_str.clone()));

        let mut collection = BuildingCollection {
            buildings: Vec::new(),
            default_storey_height,
            geo_core,
            filepath_shp,
            #[cfg(not(feature = "wasm"))]
            ign_collect: None,
        };

        // Initialize IgnCollect if no shapefile provided (will be used for IGN API)
        #[cfg(not(feature = "wasm"))]
        if collection.filepath_shp.is_none() {
            collection.ign_collect = Some(IgnCollect::new()?);
        }

        Ok(collection)
    }

    /// Create a new BuildingCollection with default parameters
    /// Convenience method for backward compatibility
    pub fn new_simple(output_path: Option<String>) -> Self {
        use crate::collect::global_variables::TEMP_PATH;

        let output_path_str = output_path.unwrap_or_else(|| TEMP_PATH.to_string());
        let mut geo_core = GeoCore::default();
        geo_core.set_output_path(Some(output_path_str));

        BuildingCollection {
            buildings: Vec::new(),
            default_storey_height: 3.0,
            geo_core,
            filepath_shp: None,
            #[cfg(not(feature = "wasm"))]
            ign_collect: None,
        }
    }

    /// Set default storey height
    pub fn set_default_storey_height(&mut self, height: f64) {
        self.default_storey_height = height;
    }

    /// Set CRS
    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core = GeoCore::new(epsg);
    }

    /// Calculate mean district height (weighted by area)
    /// Following Python: mean_distric_height = gdf["areaHauteur"].sum() / (gdf["area"].sum())
    /// Only uses buildings that already have a height (not null)
    pub fn calculate_mean_height(&self) -> f64 {
        // Calculate sum of (area * height) for buildings with height
        // Python: gdf["areaHauteur"] = gdf.apply(lambda x: x["area"] * x["hauteur"], axis=1)
        let total_area_height: f64 = self
            .buildings
            .iter()
            .filter_map(|b| {
                // Only use buildings with existing height (not null)
                b.height.map(|h| b.area * h)
            })
            .sum();

        // Calculate sum of areas for buildings with height
        let total_area: f64 = self
            .buildings
            .iter()
            .filter(|b| b.height.is_some())
            .map(|b| b.area)
            .sum();

        if total_area > 0.0 {
            total_area_height / total_area
        } else {
            0.0
        }
    }

    /// Process heights: fill missing heights using defaults or mean
    /// Following Python implementation:
    /// 1. Calculate mean district height (weighted by area) from buildings with height
    /// 2. For buildings with no height:
    ///    - If nombre_d_etages exists: use nombre_d_etages * defaultStoreyHeight
    ///    - Else if HAUTEUR_2 exists: use HAUTEUR_2
    ///    - Else: use mean district height
    /// 3. Remove buildings with no height after processing (dropna)
    pub fn process_heights(&mut self) {
        // Calculate mean height BEFORE processing (only from buildings with existing height)
        // Python: mean_distric_height = gdf["areaHauteur"].sum() / (gdf["area"].sum())
        let mean_height = self.calculate_mean_height();

        for building in &mut self.buildings {
            // Python: if x["noHauteur"] and not x["etage_nulle"]: use nombre_d_etages
            if building.no_hauteur {
                // Python: if "nombre_d_etages" in gdf.columns:
                //         gdf["hauteur"] = gdf.apply(lambda x: (
                //             x["nombre_d_etages"] * self.defaultStoreyHeight
                //             if x["noHauteur"] and not x["etage_nulle"]
                //             else x["hauteur"]
                //         ), axis=1)
                if let Some(etages) = building.nombre_d_etages {
                    building.set_height(etages * self.default_storey_height);
                }
                // Python: elif "HAUTEUR_2" in gdf.columns:
                //         gdf["hauteur"] = gdf.apply(lambda x: (
                //             x["HAUTEUR_2"] if x["noHauteur"] and not x["noH2"] else x["hauteur"]
                //         ), axis=1)
                else if let Some(h2) = building.hauteur_2 {
                    building.set_height(h2);
                }
                // Python: else:
                //         gdf["hauteur"] = gdf.apply(lambda x: mean_distric_height if x["noHauteur"] else x["hauteur"], axis=1)
                else {
                    building.set_height(mean_height);
                }
            }
        }

        // Python: gdf.dropna(subset=["hauteur"], inplace=True)
        // Remove buildings with no height after processing
        self.buildings.retain(|b| b.height.is_some());
    }

    /// Add a building to the collection
    pub fn add_building(&mut self, building: Building) {
        self.buildings.push(building);
    }

    /// Get number of buildings
    pub fn len(&self) -> usize {
        self.buildings.len()
    }

    /// Check if collection is empty
    pub fn is_empty(&self) -> bool {
        self.buildings.is_empty()
    }

    /// Load buildings from a Shapefile
    /// NOTE: Temporarily disabled due to GDAL API issues
    /// TODO: Fix GDAL integration
    #[allow(dead_code)]
    pub fn from_shapefile<P: AsRef<Path>>(
        _filepath: P,
        _output_path: Option<String>,
        _default_storey_height: f64,
        _set_crs: Option<i32>,
    ) -> Result<Self> {
        anyhow::bail!("Shapefile loading is temporarily disabled. Use from_geojson() instead.");
        /*
        let mut collection = Self::new(Some(filepath.as_ref().to_string_lossy().to_string()), output_path, default_storey_height, set_crs)?;

        let dataset = Dataset::open(filepath.as_ref())
            .context("Failed to open shapefile")?;

        let layer = dataset.layer(0)
            .context("Failed to get layer from dataset")?;

        // Handle CRS
        if let Some(epsg) = set_crs {
            collection.set_crs(epsg);
        } else if let Some(srs) = layer.spatial_ref() {
            if let Some(epsg_code) = srs.to_epsg() {
                collection.set_crs(epsg_code);
            }
        }

        // Iterate through features
        for (idx, feature) in layer.features().enumerate() {
            let feature = feature?;

            if let Some(building) = Self::feature_to_building(&feature, default_storey_height)? {
                collection.add_building(building);
            }
        }

        Ok(collection)
        */
    }

    /// Load buildings from GeoJSON (file or bytes)
    pub fn from_geojson(
        geojson_data: &[u8],
        output_path: Option<String>,
        default_storey_height: f64,
        set_crs: Option<i32>,
    ) -> Result<Self> {
        let mut collection = Self::new(None, output_path, default_storey_height, set_crs)?;

        // Parse GeoJSON using geojson crate
        let geojson_str =
            std::str::from_utf8(geojson_data).context("GeoJSON data is not valid UTF-8")?;
        let geojson: GeoJson = geojson_str.parse().context("Failed to parse GeoJSON")?;

        // Extract features
        match geojson {
            GeoJson::FeatureCollection(fc) => {
                for feature in fc.features {
                    // Skip features that are not polygons (continue processing)
                    match Self::geojson_feature_to_building(&feature, default_storey_height) {
                        Ok(Some(building)) => {
                            collection.add_building(building);
                        }
                        Ok(None) => {
                            // Not a polygon - skip and continue
                            continue;
                        }
                        Err(e) => {
                            // Log error but continue processing other features
                            eprintln!("Warning: Failed to process feature: {}", e);
                            continue;
                        }
                    }
                }
            }
            GeoJson::Feature(f) => {
                // Skip features that are not polygons
                match Self::geojson_feature_to_building(&f, default_storey_height) {
                    Ok(Some(building)) => {
                        collection.add_building(building);
                    }
                    Ok(None) => {
                        // Not a polygon - skip
                    }
                    Err(e) => {
                        // Log error but don't fail completely
                        eprintln!("Warning: Failed to process feature: {}", e);
                    }
                }
            }
            _ => {
                anyhow::bail!("GeoJSON must be a Feature or FeatureCollection");
            }
        }

        Ok(collection)
    }

    /// Load buildings from IGN API
    pub fn from_ign_api(
        output_path: Option<String>,
        default_storey_height: f64,
        bbox: Option<BoundingBox>,
    ) -> Result<Self> {
        let mut ign_collect = IgnCollect::new()?;

        if let Some(bbox) = bbox {
            ign_collect.bbox = Some(bbox);
        } else {
            anyhow::bail!("Bounding box is required for IGN API requests");
        }

        // Execute IGN API request
        ign_collect.execute_ign("buildings")?;

        // Get GeoJSON content as bytes
        let geojson_bytes = ign_collect
            .content
            .as_ref()
            .context("No content available from IGN API")?;

        Self::from_geojson(geojson_bytes, output_path, default_storey_height, None)
    }

    /// Convert a GDAL feature to a Building
    /// NOTE: Temporarily disabled due to GDAL API issues
    #[allow(dead_code)]
    fn feature_to_building(
        _feature: &GdalFeature,
        _default_storey_height: f64,
    ) -> Result<Option<Building>> {
        anyhow::bail!("GDAL feature conversion is temporarily disabled");
        /*
        // Get geometry
        let geometry = feature.geometry()
            .context("Feature has no geometry")?;

        // Convert GDAL geometry to geo::Polygon
        let polygon = Self::gdal_geometry_to_polygon(&geometry)?;

        let mut building = Building::new(polygon);

        // Extract attributes
        for field_idx in 0..feature.field_count() {
            let field_defn = feature.field_defn(field_idx)
                .context("Failed to get field definition")?;

            let field_name = field_defn.name();
            let field_value = feature.field(field_idx);

            match field_name.to_lowercase().as_str() {
                "hauteur" | "height" => {
                    if let Some(f) = field_value.as_real() {
                        if f.is_finite() && f > 0.0 {
                            building.set_height(f);
                        }
                    }
                }
                "nombre_d_etages" | "storeys" | "etages" => {
                    if let Some(f) = field_value.as_real() {
                        if f.is_finite() && f > 0.0 {
                            building.set_nombre_d_etages(f);
                        }
                    }
                }
                "hauteur_2" | "height_2" | "h2" => {
                    if let Some(f) = field_value.as_real() {
                        if f.is_finite() && f > 0.0 {
                            building.set_hauteur_2(f);
                        }
                    }
                }
                _ => {
                    // Store other fields as metadata
                    if let Some(s) = field_value.as_string() {
                        building.metadata.insert(field_name.to_string(), s);
                    }
                }
            }
        }

        Ok(Some(building))
        */
    }

    /// Convert GeoJSON feature to Building
    /// Returns None if geometry is not a polygon (allows skipping non-polygon features)
    fn geojson_feature_to_building(
        feature: &GeoJsonFeature,
        _default_storey_height: f64,
    ) -> Result<Option<Building>> {
        let geometry = feature
            .geometry
            .as_ref()
            .context("Feature has no geometry")?;

        // Get polygon from geometry (handles Polygon and MultiPolygon)
        // Returns None if not a polygon type - we skip these features
        let polygon = match Self::geojson_geometry_to_polygon(geometry)? {
            Some(poly) => poly,
            None => {
                // Not a polygon - skip this feature
                return Ok(None);
            }
        };

        let mut building = Building::new(polygon);

        // Extract properties
        if let Some(properties) = &feature.properties {
            for (key, value) in properties {
                match key.to_lowercase().as_str() {
                    "hauteur" | "height" => {
                        if let Some(f) = value.as_f64() {
                            if f.is_finite() && f > 0.0 {
                                building.set_height(f);
                            }
                        } else if let Some(n) = value.as_i64() {
                            if n > 0 {
                                building.set_height(n as f64);
                            }
                        }
                    }
                    "nombre_d_etages" | "storeys" | "etages" => {
                        if let Some(f) = value.as_f64() {
                            if f.is_finite() && f > 0.0 {
                                building.set_nombre_d_etages(f);
                            }
                        } else if let Some(n) = value.as_i64() {
                            if n > 0 {
                                building.set_nombre_d_etages(n as f64);
                            }
                        }
                    }
                    "hauteur_2" | "height_2" | "h2" => {
                        // Python uses "HAUTEUR_2" (uppercase), but we also check lowercase variants
                        // Following Python: elif "HAUTEUR_2" in gdf.columns
                        if let Some(f) = value.as_f64() {
                            if f.is_finite() && f > 0.0 {
                                building.set_hauteur_2(f);
                            }
                        } else if let Some(n) = value.as_i64() {
                            if n > 0 {
                                building.set_hauteur_2(n as f64);
                            }
                        }
                    }
                    _ => {
                        if let Some(s) = value.as_str() {
                            building.metadata.insert(key.clone(), s.to_string());
                        } else if let Some(f) = value.as_f64() {
                            building.metadata.insert(key.clone(), f.to_string());
                        } else if let Some(n) = value.as_i64() {
                            building.metadata.insert(key.clone(), n.to_string());
                        }
                    }
                }
            }
        }

        Ok(Some(building))
    }

    /// Convert GDAL geometry to geo::Polygon
    /// NOTE: Temporarily disabled due to GDAL API issues
    #[allow(dead_code)]
    fn gdal_geometry_to_polygon(_geometry: &gdal::vector::Geometry) -> Result<Polygon<f64>> {
        anyhow::bail!("GDAL geometry conversion is temporarily disabled");
        /*
        // Get WKT representation and parse it
        let wkt = geometry.wkt()
            .context("Failed to get WKT from geometry")?;

        // Use geos to parse WKT (more reliable than manual parsing)
        let geos_geom = geos::Geometry::try_from(geometry)
            .context("Failed to convert GDAL geometry to GEOS")?;

        // Convert GEOS to geo
        let geo_geom: geo::Geometry<f64> = geos_geom.try_into()
            .context("Failed to convert GEOS geometry to geo")?;

        match geo_geom {
            geo::Geometry::Polygon(poly) => Ok(poly),
            _ => anyhow::bail!("Geometry is not a polygon"),
        }
        */
    }

    /// Convert GeoJSON geometry to Polygon
    /// Handles Polygon and MultiPolygon (takes first polygon from MultiPolygon)
    /// Returns None if geometry is not a polygon type (allows skipping non-polygon features)
    fn geojson_geometry_to_polygon(geometry: &Geometry) -> Result<Option<Polygon<f64>>> {
        // Convert geojson::Geometry to geo::Geometry
        let geo_geom: geo::Geometry<f64> = geometry
            .try_into()
            .context("Failed to convert GeoJSON geometry to geo::Geometry")?;

        match geo_geom {
            geo::Geometry::Polygon(poly) => Ok(Some(poly)),
            geo::Geometry::MultiPolygon(mp) => {
                // Take the first polygon from MultiPolygon
                if let Some(first_poly) = mp.0.first() {
                    Ok(Some(first_poly.clone()))
                } else {
                    Ok(None) // Empty MultiPolygon
                }
            }
            _ => {
                // Not a polygon type - return None to skip this feature
                Ok(None)
            }
        }
    }

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
        // CRS is already handled in from_geojson() or would be handled in from_shapefile()

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

    /// Convert building collection to Polars DataFrame
    /// Similar to to_gdf() in Python version
    /// Returns a standard Polars DataFrame (not GeoDataFrame)
    #[cfg(not(feature = "wasm"))]
    pub fn to_polars_df(&self) -> Result<polars::prelude::DataFrame> {
        let mut height_vec: Vec<Option<f64>> = Vec::new();
        let mut area_vec: Vec<f64> = Vec::new();
        let mut centroid_x_vec: Vec<f64> = Vec::new();
        let mut centroid_y_vec: Vec<f64> = Vec::new();
        let mut nombre_d_etages_vec: Vec<Option<f64>> = Vec::new();
        let mut hauteur_2_vec: Vec<Option<f64>> = Vec::new();
        let mut no_hauteur_vec: Vec<bool> = Vec::new();

        for building in &self.buildings {
            height_vec.push(building.height);
            area_vec.push(building.area);
            centroid_x_vec.push(building.centroid.x());
            centroid_y_vec.push(building.centroid.y());
            nombre_d_etages_vec.push(building.nombre_d_etages);
            hauteur_2_vec.push(building.hauteur_2);
            no_hauteur_vec.push(building.no_hauteur);
        }

        let df = df! [
            "hauteur" => height_vec,
            "area" => area_vec,
            "centroid_x" => centroid_x_vec,
            "centroid_y" => centroid_y_vec,
            "nombre_d_etages" => nombre_d_etages_vec,
            "hauteur_2" => hauteur_2_vec,
            "noHauteur" => no_hauteur_vec,
        ]
        .context("Failed to create DataFrame")?;

        Ok(df)
    }

    /// Convert building collection to Polars DataFrame with geometry column (EWKB format)
    /// Compatible with polars-st (https://github.com/Oreilles/polars-st)
    /// Returns a DataFrame with a "geometry" column containing EWKB bytes with SRID
    /// polars-st stores geometries as EWKB in Binary columns
    #[cfg(not(feature = "wasm"))]
    pub fn to_gdf(&self) -> Result<polars::prelude::DataFrame> {
        use std::io::{Cursor, Read, Write};

        let mut height_vec: Vec<Option<f64>> = Vec::new();
        let mut area_vec: Vec<f64> = Vec::new();
        let mut centroid_x_vec: Vec<f64> = Vec::new();
        let mut centroid_y_vec: Vec<f64> = Vec::new();
        let mut nombre_d_etages_vec: Vec<Option<f64>> = Vec::new();
        let mut hauteur_2_vec: Vec<Option<f64>> = Vec::new();
        let mut no_hauteur_vec: Vec<bool> = Vec::new();
        let mut geometry_vec: Vec<Vec<u8>> = Vec::new();

        let srid = self.geo_core.epsg;

        for building in &self.buildings {
            height_vec.push(building.height);
            area_vec.push(building.area);
            centroid_x_vec.push(building.centroid.x());
            centroid_y_vec.push(building.centroid.y());
            nombre_d_etages_vec.push(building.nombre_d_etages);
            hauteur_2_vec.push(building.hauteur_2);
            no_hauteur_vec.push(building.no_hauteur);

            // Convert Polygon to EWKB for polars-st
            // EWKB = WKB + SRID in header
            // Create WKT directly from Polygon, then use GDAL to get WKB
            let poly = &building.footprint;

            // Create WKT string from Polygon
            let mut wkt = String::from("POLYGON((");
            for (i, coord) in poly.exterior().0.iter().enumerate() {
                if i > 0 {
                    wkt.push_str(", ");
                }
                wkt.push_str(&format!("{} {}", coord.x, coord.y));
            }
            wkt.push_str(")");

            // Add interior rings if any
            for ring in poly.interiors() {
                wkt.push_str(", (");
                for (i, coord) in ring.0.iter().enumerate() {
                    if i > 0 {
                        wkt.push_str(", ");
                    }
                    wkt.push_str(&format!("{} {}", coord.x, coord.y));
                }
                wkt.push_str(")");
            }
            wkt.push_str(")");

            // Create GDAL geometry from WKT
            use gdal::vector::Geometry as GdalGeometry;
            let gdal_geom =
                GdalGeometry::from_wkt(&wkt).context("Failed to create GDAL geometry from WKT")?;

            // Get WKB from GDAL
            let wkb = gdal_geom
                .wkb()
                .context("Failed to get WKB from GDAL geometry")?;

            // Convert to EWKB by adding SRID
            // EWKB format: byte_order (1) + geometry_type_with_srid_flag (4) + srid (4) + wkb_data
            let mut ewkb = Vec::new();
            let mut cursor = Cursor::new(&wkb);

            // Read byte order (1 byte)
            let mut byte_order = [0u8; 1];
            cursor
                .read_exact(&mut byte_order)
                .context("Failed to read WKB byte order")?;
            ewkb.write_all(&byte_order)?;

            // Read geometry type (4 bytes)
            let mut geom_type_bytes = [0u8; 4];
            cursor
                .read_exact(&mut geom_type_bytes)
                .context("Failed to read WKB geometry type")?;

            // Add SRID flag (0x20000000) to geometry type
            let geom_type = u32::from_le_bytes(geom_type_bytes);
            let geom_type_with_srid = geom_type | 0x20000000;
            ewkb.write_all(&geom_type_with_srid.to_le_bytes())?;

            // Add SRID (4 bytes, little-endian)
            ewkb.write_all(&srid.to_le_bytes())?;

            // Copy remaining WKB data
            let mut remaining = Vec::new();
            cursor
                .read_to_end(&mut remaining)
                .context("Failed to read remaining WKB data")?;
            ewkb.write_all(&remaining)?;

            geometry_vec.push(ewkb);
        }

        // Create Series with geometries (EWKB bytes)
        // Polars Binary type: use df! macro which handles types automatically
        // Convert Vec<Vec<u8>> to Vec<Option<Vec<u8>>> for Binary type
        let geometry_opt_vec: Vec<Option<Vec<u8>>> = geometry_vec.into_iter().map(Some).collect();

        // Create DataFrame with all columns including geometry
        let df = df! [
            "geometry" => geometry_opt_vec,
            "hauteur" => height_vec,
            "area" => area_vec,
            "centroid_x" => centroid_x_vec,
            "centroid_y" => centroid_y_vec,
            "nombre_d_etages" => nombre_d_etages_vec,
            "hauteur_2" => hauteur_2_vec,
            "noHauteur" => no_hauteur_vec,
        ]
        .context("Failed to create GeoDataFrame")?;

        Ok(df)
    }

    /// Get a reference to the buildings vector
    pub fn buildings(&self) -> &Vec<Building> {
        &self.buildings
    }

    /// Get a mutable reference to the buildings vector
    pub fn buildings_mut(&mut self) -> &mut Vec<Building> {
        &mut self.buildings
    }

    /// Get the GeoJSON (equivalent to to_gdf() in Python)
    /// Following Python: def to_gdf(self) -> gpd.GeoDataFrame
    /// Converts the building collection to a GeoJSON FeatureCollection
    pub fn get_geojson(&self) -> Result<GeoJson> {
        use geojson::{Feature, FeatureCollection};

        let mut features = Vec::new();

        for building in &self.buildings {
            // Convert geo::Polygon to geojson::Geometry
            // First convert to geo::Geometry, then to geojson::Geometry using reference
            let geo_geom: geo::Geometry<f64> = building.footprint.clone().into();
            let geometry: Geometry = (&geo_geom)
                .try_into()
                .context("Failed to convert polygon to GeoJSON geometry")?;

            // Create feature with geometry
            let mut feature = Feature::from(geometry);

            // Add properties
            if let Some(height) = building.height {
                feature.set_property("hauteur", height);
            }
            feature.set_property("area", building.area);
            feature.set_property("centroid_x", building.centroid.x());
            feature.set_property("centroid_y", building.centroid.y());

            if let Some(etages) = building.nombre_d_etages {
                feature.set_property("nombre_d_etages", etages);
            }

            if let Some(h2) = building.hauteur_2 {
                feature.set_property("HAUTEUR_2", h2);
            }

            feature.set_property("noHauteur", building.no_hauteur);

            // Add metadata as properties
            for (key, value) in &building.metadata {
                feature.set_property(key, value.clone());
            }

            features.push(feature);
        }

        let feature_collection = FeatureCollection {
            bbox: None,
            foreign_members: None,
            features,
        };

        Ok(GeoJson::from(feature_collection))
    }

    /// Export buildings to GeoJSON file
    /// NOTE: Temporarily disabled due to GDAL API issues
    /// TODO: Fix GDAL integration
    #[allow(dead_code)]
    pub fn to_geojson<P: AsRef<Path>>(&self, _filepath: P, _name: Option<&str>) -> Result<()> {
        anyhow::bail!("GeoJSON export is temporarily disabled. Use to_polars_df() instead.");
        /*
        use gdal::vector::{Geometry, OGRFieldType};
        use std::ffi::CString;
        use std::path::PathBuf;

        let filepath = filepath.as_ref();
        let layer_name = name.unwrap_or("batiments");

        // Get or create driver
        let driver = DriverManager::get_driver_by_name("GeoJSON")
            .context("GeoJSON driver not available")?;

        // Create or open dataset
        let mut dataset = if filepath.exists() {
            Dataset::open(filepath)
                .context("Failed to open existing GeoJSON file")?
        } else {
            driver.create(filepath)
                .context("Failed to create GeoJSON file")?
        };

        // Create layer
        let mut layer = dataset.create_layer(
            gdal::vector::LayerOptions {
                name: layer_name,
                srs: None, // Will set CRS if available
                geom_type: gdal::vector::OGRwkbGeometryType::wkbPolygon,
                options: None,
            },
        )
        .context("Failed to create layer")?;

        // Set CRS if available
        if let Ok(srs) = gdal::spatial_ref::SpatialRef::from_epsg(self.geo_core.get_epsg()) {
            layer.set_spatial_ref(&srs)
                .context("Failed to set spatial reference")?;
        }

        // Create fields
        layer.create_defn_field(
            &gdal::vector::FieldDefn::new("hauteur", OGRFieldType::OFTReal)
                .context("Failed to create hauteur field")?,
        )
        .context("Failed to add hauteur field")?;

        layer.create_defn_field(
            &gdal::vector::FieldDefn::new("area", OGRFieldType::OFTReal)
                .context("Failed to create area field")?,
        )
        .context("Failed to add area field")?;

        layer.create_defn_field(
            &gdal::vector::FieldDefn::new("centroid_x", OGRFieldType::OFTReal)
                .context("Failed to create centroid_x field")?,
        )
        .context("Failed to add centroid_x field")?;

        layer.create_defn_field(
            &gdal::vector::FieldDefn::new("centroid_y", OGRFieldType::OFTReal)
                .context("Failed to create centroid_y field")?,
        )
        .context("Failed to add centroid_y field")?;

        // Add buildings as features
        for building in &self.buildings {
            let mut feature = layer.create_feature(
                layer.defn()
                    .context("Failed to get layer definition")?,
            )
            .context("Failed to create feature")?;

            // Convert geo::Polygon to GDAL Geometry
            let geos_geom: geos::Geometry = building.footprint.clone().try_into()
                .context("Failed to convert polygon to GEOS")?;

            let gdal_geom: gdal::Geometry = geos_geom.try_into()
                .context("Failed to convert GEOS to GDAL geometry")?;

            feature.set_geometry(gdal_geom)
                .context("Failed to set geometry")?;

            // Set fields
            if let Some(height) = building.height {
                feature.set_field_double_by_name("hauteur", height)
                    .context("Failed to set hauteur field")?;
            }

            feature.set_field_double_by_name("area", building.area)
                .context("Failed to set area field")?;

            feature.set_field_double_by_name("centroid_x", building.centroid.x())
                .context("Failed to set centroid_x field")?;

            feature.set_field_double_by_name("centroid_y", building.centroid.y())
                .context("Failed to set centroid_y field")?;

            feature.create(layer)
                .context("Failed to create feature in layer")?;
        }

        Ok(())
        */
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::polygon;

    #[test]
    fn test_building_new() {
        let poly = polygon![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0),
        ];
        let building = Building::new(poly);
        assert!(building.height.is_none());
        assert!(building.area > 0.0);
    }

    #[test]
    fn test_building_with_height() {
        let poly = polygon![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0),
        ];
        let building = Building::with_height(poly, 10.0);
        assert_eq!(building.height, Some(10.0));
    }

    #[test]
    fn test_building_collection() {
        let mut collection = BuildingCollection::new_simple(None);
        assert_eq!(collection.len(), 0);

        let poly = polygon![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0),
        ];
        let building = Building::with_height(poly, 10.0);
        collection.add_building(building);
        assert_eq!(collection.len(), 1);
    }

    #[test]
    fn test_calculate_mean_height() {
        let mut collection = BuildingCollection::new_simple(None);

        // Add buildings with different heights
        let poly1 = polygon![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0),
        ];
        let building1 = Building::with_height(poly1, 10.0);
        collection.add_building(building1);

        let poly2 = polygon![
            (x: 2.0, y: 0.0),
            (x: 3.0, y: 0.0),
            (x: 3.0, y: 1.0),
            (x: 2.0, y: 1.0),
            (x: 2.0, y: 0.0),
        ];
        let building2 = Building::with_height(poly2, 20.0);
        collection.add_building(building2);

        let mean_height = collection.calculate_mean_height();
        // Both buildings have same area, so mean should be (10 + 20) / 2 = 15
        assert!((mean_height - 15.0).abs() < 0.001);
    }

    #[test]
    fn test_process_heights() {
        let mut collection = BuildingCollection::new_simple(None);
        collection.set_default_storey_height(3.0);

        // Building with no height but with storeys
        let poly1 = polygon![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0),
        ];
        let mut building1 = Building::new(poly1);
        building1.set_nombre_d_etages(5.0);
        collection.add_building(building1);

        // Building with height
        let poly2 = polygon![
            (x: 2.0, y: 0.0),
            (x: 3.0, y: 0.0),
            (x: 3.0, y: 1.0),
            (x: 2.0, y: 1.0),
            (x: 2.0, y: 0.0),
        ];
        let building2 = Building::with_height(poly2, 12.0);
        collection.add_building(building2);

        collection.process_heights();

        // First building should have height = 5 * 3 = 15
        assert_eq!(collection.buildings[0].height, Some(15.0));
        // Second building should keep its height
        assert_eq!(collection.buildings[1].height, Some(12.0));
    }

    #[test]
    fn test_to_polars_df() {
        let mut collection = BuildingCollection::new_simple(None);

        let poly = polygon![
            (x: 0.0, y: 0.0),
            (x: 1.0, y: 0.0),
            (x: 1.0, y: 1.0),
            (x: 0.0, y: 1.0),
            (x: 0.0, y: 0.0),
        ];
        let building = Building::with_height(poly, 10.0);
        collection.add_building(building);

        let df = collection.to_polars_df().unwrap();
        assert_eq!(df.height(), 1);
        assert!(df.column("hauteur").is_ok());
        assert!(df.column("area").is_ok());
    }
}
