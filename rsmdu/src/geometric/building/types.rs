//! Building types and collection basics.

use anyhow::Result;
use geo::{Area, Centroid, Polygon};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(not(feature = "wasm"))]
use crate::collect::ign::ign_collect::IgnCollect;
use crate::geo_core::GeoCore;

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
    pub(crate) ign_collect: Option<IgnCollect>,
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
}
