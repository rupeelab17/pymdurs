use anyhow::{Context, Result};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, GeometryValue};
use serde_json::Map;
use std::path::{Path, PathBuf};

use crate::geo_core::{BoundingBox, GeoCore};

/// RNB (Référentiel National des Bâtiments) structure
/// Following Python implementation from pymdu.geometric.Rnb
/// Provides methods to collect and process RNB building data from RNB API
pub struct Rnb {
    /// Output path for processed data
    output_path: PathBuf,
    /// GeoCore for CRS handling
    pub geo_core: GeoCore,
    /// Bounding box for the RNB area
    bbox: Option<BoundingBox>,
    /// Parsed GeoJSON content
    geojson: Option<GeoJson>,
}

/// RNB API response structure
#[derive(serde::Deserialize)]
struct RnbApiResponse {
    results: Vec<RnbBuilding>,
}

/// RNB Building structure from API
#[derive(serde::Deserialize)]
struct RnbBuilding {
    rnb_id: String,
    status: String,
    point: RnbPoint,
    addresses: Vec<RnbAddress>,
    ext_ids: Vec<RnbExtId>,
}

/// RNB Point structure
#[derive(serde::Deserialize)]
struct RnbPoint {
    coordinates: Vec<f64>, // [lon, lat]
}

/// RNB Address structure
#[derive(serde::Deserialize)]
struct RnbAddress {
    street_number: Option<String>,
    city_name: Option<String>,
    city_zipcode: Option<String>,
}

/// RNB External ID structure
#[derive(serde::Deserialize)]
struct RnbExtId {
    created_at: Option<String>,
}

impl Rnb {
    /// Create a new Rnb instance
    /// Following Python: def __init__(self, output_path: str | None = None)
    pub fn new(output_path: Option<String>) -> Result<Self> {
        use crate::collect::global_variables::get_temp_path;

        let output_path_buf = output_path.map(PathBuf::from).unwrap_or_else(get_temp_path);

        Ok(Rnb {
            output_path: output_path_buf,
            geo_core: GeoCore::default(), // Default to EPSG:2154 (Lambert-93)
            bbox: None,
            geojson: None,
        })
    }

    /// Set bounding box
    /// Following Python: rnb.bbox = [min_x, min_y, max_x, max_y]
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
    }

    /// Set CRS
    /// Following Python: rnb._epsg = epsg
    pub fn set_crs(&mut self, epsg: i32) {
        self.geo_core.set_epsg(epsg);
    }

    /// Run RNB processing: fetch from RNB API, parse JSON, create GeoJSON
    /// Following Python: def run(self) -> self
    pub fn run(mut self) -> Result<Self> {
        self.run_internal()?;
        Ok(self)
    }

    /// Internal run method that can be called mutably
    /// Used by Python bindings to avoid ownership issues
    pub fn run_internal(&mut self) -> Result<()> {
        let bbox = self
            .bbox
            .as_ref()
            .context("Bounding box must be set before running RNB")?;

        // Python: url = "https://rnb-api.beta.gouv.fr/api/alpha/buildings"
        let url = "https://rnb-api.beta.gouv.fr/api/alpha/buildings";

        // According to RNB API documentation:
        // - bbox (recommended): min_lon,min_lat,max_lon,max_lat
        // - bb (obsolete): nw_lat,nw_lon,se_lat,se_lon
        // Python uses bb with format: min_y, min_x, max_y, max_x
        // We'll use the recommended bbox parameter with format: min_lon,min_lat,max_lon,max_lat
        // Which corresponds to: min_x, min_y, max_x, max_y
        let bbox_param = format!(
            "{},{},{},{}",
            bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y
        );
        tracing::info!("RNB API bbox parameter: {}", bbox_param);
        tracing::info!("RNB API URL: {}", url);

        // Python: requests.get(..., verify=False)
        let client = crate::commons::http::rnb()?;
        let response = client
            .get(url)
            .header("Content-type", "application/json")
            .query(&[("bbox", &bbox_param)])
            .send()
            .context("Failed to send request to RNB API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("RNB API returned error {}: {}", status, body);
        }

        // Parse JSON response
        // Python: content = response.json()
        let api_response: RnbApiResponse = response
            .json()
            .context("Failed to parse JSON response from RNB API")?;

        // Convert to GeoJSON FeatureCollection
        // Python: for item in content["results"]:
        //         coordinates = item["point"]["coordinates"]
        //         geometry = [Point(coordinates)]
        //         ...
        //         gdf = gpd.GeoDataFrame(df, geometry=geometry, crs="EPSG:4326")
        //         gdf = gdf.to_crs(self._epsg)
        let mut features = Vec::new();

        for building in api_response.results {
            // Extract coordinates
            if building.point.coordinates.len() < 2 {
                continue; // Skip invalid coordinates
            }
            let lon = building.point.coordinates[0];
            let lat = building.point.coordinates[1];

            // Create Point geometry
            // Python: geometry = [Point(coordinates)]
            let geometry = Geometry::new(GeometryValue::new_point([lon, lat]));

            // Create properties
            let mut properties = Map::new();
            properties.insert(
                "rnb_id".to_string(),
                serde_json::Value::String(building.rnb_id),
            );
            properties.insert(
                "status".to_string(),
                serde_json::Value::String(building.status),
            );

            // Add address information if available
            // Python: if len(item["addresses"]) > 0:
            if let Some(address) = building.addresses.first() {
                if let Some(ref street_number) = address.street_number {
                    let street_number_str: String = street_number.clone();
                    properties.insert(
                        "street_number".to_string(),
                        serde_json::Value::String(street_number_str),
                    );
                }
                if let Some(ref city_name) = address.city_name {
                    let city_name_str: String = city_name.clone();
                    properties.insert(
                        "city_name".to_string(),
                        serde_json::Value::String(city_name_str),
                    );
                }
                if let Some(ref city_zipcode) = address.city_zipcode {
                    let city_zipcode_str: String = city_zipcode.clone();
                    properties.insert(
                        "city_zipcode".to_string(),
                        serde_json::Value::String(city_zipcode_str),
                    );
                }
            }

            // Add created_at if available
            // Python: created_at = item["ext_ids"][0]["created_at"]
            if let Some(ext_id) = building.ext_ids.first() {
                if let Some(ref created_at) = ext_id.created_at {
                    let created_at_str: String = created_at.clone();
                    properties.insert(
                        "created_at".to_string(),
                        serde_json::Value::String(created_at_str),
                    );
                }
            }

            // Create feature
            let mut feature = Feature::from(geometry);
            feature.properties = Some(properties);

            features.push(feature);
        }

        // Create FeatureCollection
        let feature_collection = FeatureCollection {
            bbox: None,
            foreign_members: None,
            features,
        };

        // Note: Reprojection to target CRS (Python: gdf = gdf.to_crs(self._epsg))
        // would require converting GeoJSON to GDAL Dataset, reprojecting, and converting back
        // This is complex and would require additional dependencies
        // For now, we store the GeoJSON as-is in EPSG:4326
        // TODO: Implement reprojection using GDAL or proj crate
        self.geojson = Some(GeoJson::from(feature_collection));

        Ok(())
    }

    /// Get the GeoJSON (equivalent to to_gdf() in Python)
    /// Following Python: def to_gdf(self) -> gpd.GeoDataFrame
    pub fn get_geojson(&self) -> Option<&GeoJson> {
        self.geojson.as_ref()
    }

    /// Save to GeoJSON file
    /// Following Python: def to_geojson(self, name: str = "rnb")
    /// Note: GeoJSON export requires GDAL and is complex
    /// For now, we save as GeoJSON - full GeoJSON export would require GDAL layer operations
    /// TODO: Implement full GeoJSON export using GDAL
    pub fn to_geojson(&self, name: Option<&str>) -> Result<()> {
        // Python: self.gdf.to_file(f"{os.path.join(self.output_path, name)}.gpkg", driver="GeoJSON")
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

        let name = name.unwrap_or("rnb");

        // Save as GeoJSON for now (GeoJSON export is complex with GDAL Rust bindings)
        let output_file = self.output_path.join(format!("{}.geojson", name));
        let geojson_str = geojson.to_string();
        std::fs::write(&output_file, geojson_str)
            .context(format!("Failed to write GeoJSON file: {:?}", output_file))?;

        tracing::info!(
            "RNB saved to: {:?} (as GeoJSON - GeoJSON export temporarily disabled)",
            output_file
        );
        tracing::info!("  TODO: Implement full GeoJSON export using GDAL");

        Ok(())
    }

    /// Get output path
    pub fn get_output_path(&self) -> &Path {
        &self.output_path
    }
}
