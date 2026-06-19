use geo::{Area, Polygon};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry};
use geotiff::GeoTiff;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use wasm_bindgen::prelude::*;

/// Initialize the WASM module with panic hook
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Building structure for WASM
#[derive(Debug, Clone)]
struct WasmBuilding {
    footprint: Polygon<f64>,
    height: Option<f64>,
    area: f64,
    nombre_d_etages: Option<f64>,
    hauteur_2: Option<f64>,
    no_hauteur: bool,
}

impl WasmBuilding {
    /// Create a new building from a polygon footprint
    fn new(footprint: Polygon<f64>) -> Self {
        let area = footprint.unsigned_area();
        Self {
            footprint,
            height: None,
            area,
            nombre_d_etages: None,
            hauteur_2: None,
            no_hauteur: true,
        }
    }

    /// Set the building height
    fn set_height(&mut self, height: f64) {
        if height > 0.0 {
            self.height = Some(height);
            self.no_hauteur = false;
        }
    }

    /// Set the number of storeys
    fn set_nombre_d_etages(&mut self, etages: f64) {
        if etages > 0.0 {
            self.nombre_d_etages = Some(etages);
        }
    }

    /// Set alternative height
    fn set_hauteur_2(&mut self, hauteur: f64) {
        if hauteur > 0.0 {
            self.hauteur_2 = Some(hauteur);
        }
    }

    /// Get effective height (with fallback logic)
    fn get_effective_height(&self, default_storey_height: f64, mean_height: f64) -> Option<f64> {
        self.height
            .or_else(|| self.nombre_d_etages.map(|e| e * default_storey_height))
            .or(self.hauteur_2)
            .or_else(|| {
                if mean_height > 0.0 {
                    Some(mean_height)
                } else {
                    None
                }
            })
    }
}

/// Building collection wrapper for WASM
#[wasm_bindgen]
pub struct WasmBuildingCollection {
    buildings: Vec<WasmBuilding>,
    default_storey_height: f64,
}

#[wasm_bindgen]
impl WasmBuildingCollection {
    /// Create a new empty building collection
    #[wasm_bindgen(constructor)]
    pub fn new(default_storey_height: f64) -> Self {
        Self {
            buildings: Vec::new(),
            default_storey_height: default_storey_height.max(0.1), // Ensure positive
        }
    }

    /// Load buildings from IGN API
    ///
    /// # Arguments
    /// * `min_x` - Minimum longitude (west)
    /// * `min_y` - Minimum latitude (south)
    /// * `max_x` - Maximum longitude (east)
    /// * `max_y` - Maximum latitude (north)
    /// * `default_storey_height` - Default height per storey in meters (e.g., 3.0)
    ///
    /// # Returns
    /// A new WasmBuildingCollection instance
    ///
    /// # Errors
    /// Returns JsValue error if the API request fails or data is invalid
    #[wasm_bindgen]
    pub async fn from_ign_api(
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        default_storey_height: f64,
    ) -> Result<WasmBuildingCollection, JsValue> {
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode};

        // Validate coordinates
        if min_x >= max_x || min_y >= max_y {
            return Err(JsValue::from_str(
                "Invalid bounding box: min values must be less than max values",
            ));
        }

        // Build WFS request URL for IGN API (WFS 2.0.0 standard)
        let typename = "BDTOPO_V3:batiment";
        let base_url = "https://data.geopf.fr/wfs/ows";

        // Format bbox as: min_y,min_x,max_y,max_x (lat,lon format for EPSG:4326)
        let bbox_str = format!("{},{},{},{}", min_y, min_x, max_y, max_x);

        let request_url = format!(
            "{}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES={}&CRS=EPSG:4326&BBOX={}&OUTPUTFORMAT=application/json&STARTINDEX=0&COUNT=10000",
            base_url, typename, bbox_str
        );

        // Create and configure fetch request
        let mut opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&request_url, &opts)
            .map_err(|e| JsValue::from_str(&format!("Failed to create request: {:?}", e)))?;

        // Execute request
        let window =
            web_sys::window().ok_or_else(|| JsValue::from_str("No window object available"))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| JsValue::from_str(&format!("Network request failed: {:?}", e)))?;

        let resp: web_sys::Response = resp_value
            .dyn_into()
            .map_err(|_| JsValue::from_str("Invalid response type"))?;

        // Check response status
        if !resp.ok() {
            return Err(JsValue::from_str(&format!(
                "IGN API error {}: {}",
                resp.status(),
                resp.status_text()
            )));
        }

        // Get response text
        let text_promise = resp
            .text()
            .map_err(|e| JsValue::from_str(&format!("Failed to get response text: {:?}", e)))?;

        let text = JsFuture::from(text_promise)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to read response: {:?}", e)))?;

        let geojson_str = text
            .as_string()
            .ok_or_else(|| JsValue::from_str("Response is not a valid string"))?;

        // Parse and return collection
        Self::from_geojson(&geojson_str, default_storey_height)
    }

    /// Load buildings from GeoJSON string
    ///
    /// # Errors
    /// Returns JsValue error if GeoJSON parsing fails
    #[wasm_bindgen]
    pub fn from_geojson(
        geojson_str: &str,
        default_storey_height: f64,
    ) -> Result<WasmBuildingCollection, JsValue> {
        let geojson: GeoJson = geojson_str
            .parse()
            .map_err(|e| JsValue::from_str(&format!("Invalid GeoJSON: {}", e)))?;

        let mut collection = Self::new(default_storey_height);

        match geojson {
            GeoJson::FeatureCollection(fc) => {
                collection.buildings.reserve(fc.features.len());
                for feature in fc.features {
                    if let Some(building) = Self::feature_to_building(&feature) {
                        collection.buildings.push(building);
                    }
                }
            }
            GeoJson::Feature(f) => {
                if let Some(building) = Self::feature_to_building(&f) {
                    collection.buildings.push(building);
                }
            }
            _ => {
                return Err(JsValue::from_str(
                    "GeoJSON must be a Feature or FeatureCollection",
                ));
            }
        }

        Ok(collection)
    }

    /// Convert a GeoJSON feature to a building
    fn feature_to_building(feature: &Feature) -> Option<WasmBuilding> {
        let geometry = feature.geometry.as_ref()?;

        // Convert geojson::Geometry to geo::Polygon
        let geo_geom: geo::Geometry<f64> = geometry.try_into().ok()?;

        let polygon = match geo_geom {
            geo::Geometry::Polygon(p) => p,
            geo::Geometry::MultiPolygon(mp) => mp.0.first()?.clone(),
            _ => return None,
        };

        let mut building = WasmBuilding::new(polygon);

        // Extract properties if available
        let props = feature.properties.as_ref()?;

        // Extract height with multiple fallback property names
        Self::extract_numeric_property(props, &["hauteur", "height"])
            .and_then(|h| if h > 0.0 { Some(h) } else { None })
            .map(|h| building.set_height(h));

        // Extract number of storeys
        Self::extract_numeric_property(props, &["nombre_d_etages", "storeys", "etages"])
            .and_then(|e| if e > 0.0 { Some(e) } else { None })
            .map(|e| building.set_nombre_d_etages(e));

        // Extract alternative height
        Self::extract_numeric_property(props, &["hauteur_2", "height_2", "h2", "HAUTEUR_2"])
            .and_then(|h| if h > 0.0 { Some(h) } else { None })
            .map(|h| building.set_hauteur_2(h));

        Some(building)
    }

    /// Extract numeric property from multiple possible keys
    fn extract_numeric_property(
        props: &serde_json::Map<String, serde_json::Value>,
        keys: &[&str],
    ) -> Option<f64> {
        for key in keys {
            if let Some(value) = props.get(*key) {
                if let Some(num) = value.as_f64() {
                    return Some(num);
                }
                if let Some(int) = value.as_i64() {
                    return Some(int as f64);
                }
            }
        }
        None
    }

    /// Get the number of buildings in the collection
    #[wasm_bindgen]
    pub fn len(&self) -> usize {
        self.buildings.len()
    }

    /// Check if the collection is empty
    #[wasm_bindgen]
    pub fn is_empty(&self) -> bool {
        self.buildings.is_empty()
    }

    /// Calculate mean height (weighted by area)
    fn calculate_mean_height(&self) -> f64 {
        let (total_area_height, total_area) = self
            .buildings
            .iter()
            .filter_map(|b| b.height.map(|h| (b.area * h, b.area)))
            .fold((0.0, 0.0), |(sum_ah, sum_a), (ah, a)| {
                (sum_ah + ah, sum_a + a)
            });

        if total_area > 0.0 {
            total_area_height / total_area
        } else {
            0.0
        }
    }

    /// Process building heights (fill missing heights using defaults or mean)
    #[wasm_bindgen]
    pub fn process_heights(&mut self) {
        let mean_height = self.calculate_mean_height();

        for building in &mut self.buildings {
            if building.no_hauteur {
                if let Some(height) =
                    building.get_effective_height(self.default_storey_height, mean_height)
                {
                    building.set_height(height);
                }
            }
        }

        // Remove buildings with no valid height
        self.buildings
            .retain(|b| b.height.is_some() && b.height.unwrap() > 0.0);
    }

    /// Convert the building collection to GeoJSON string
    ///
    /// # Errors
    /// Returns JsValue error if geometry conversion fails
    #[wasm_bindgen]
    pub fn to_geojson(&self) -> Result<String, JsValue> {
        let features: Result<Vec<Feature>, JsValue> = self
            .buildings
            .iter()
            .map(|building| {
                // Convert geo::Polygon to geojson::Geometry
                let geo_geom: geo::Geometry<f64> = building.footprint.clone().into();
                let geometry: Geometry = (&geo_geom).try_into().map_err(|e| {
                    JsValue::from_str(&format!("Geometry conversion failed: {}", e))
                })?;

                let mut feature = Feature::from(geometry);

                // Add properties
                if let Some(height) = building.height {
                    feature.set_property("hauteur", height);
                }
                feature.set_property("area", building.area);

                if let Some(etages) = building.nombre_d_etages {
                    feature.set_property("nombre_d_etages", etages);
                }

                if let Some(h2) = building.hauteur_2 {
                    feature.set_property("HAUTEUR_2", h2);
                }

                feature.set_property("noHauteur", building.no_hauteur);

                Ok(feature)
            })
            .collect();

        let features = features?;

        let feature_collection = FeatureCollection {
            bbox: None,
            foreign_members: None,
            features,
        };

        Ok(GeoJson::from(feature_collection).to_string())
    }

    /// Get building statistics
    ///
    /// # Errors
    /// Returns JsValue error if serialization fails
    #[wasm_bindgen]
    pub fn get_stats(&self) -> Result<JsValue, JsValue> {
        let heights: Vec<f64> = self.buildings.iter().filter_map(|b| b.height).collect();

        let (min_height, max_height) = if heights.is_empty() {
            (0.0, 0.0)
        } else {
            (
                heights.iter().fold(f64::INFINITY, |a, &b| a.min(b)),
                heights.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)),
            )
        };

        let stats = BuildingStats {
            count: self.buildings.len(),
            total_area: self.buildings.iter().map(|b| b.area).sum(),
            mean_height: self.calculate_mean_height(),
            buildings_with_height: heights.len(),
            min_height,
            max_height,
        };

        serde_wasm_bindgen::to_value(&stats)
            .map_err(|e| JsValue::from_str(&format!("Serialization failed: {}", e)))
    }

    /// Free the building collection (explicit cleanup)
    #[wasm_bindgen]
    pub fn free(self) {
        // Explicit drop - resources are cleaned up automatically
        drop(self);
    }
}

/// Building statistics structure
#[derive(Serialize, Deserialize)]
struct BuildingStats {
    count: usize,
    total_area: f64,
    mean_height: f64,
    buildings_with_height: usize,
    min_height: f64,
    max_height: f64,
}

/// Cadastral parcels from IGN WFS (GeoJSON)
#[wasm_bindgen]
pub struct WasmCadastre {
    geojson: GeoJson,
}

#[derive(Serialize, Deserialize)]
struct CadastreStats {
    count: usize,
    total_contenance: f64,
}

#[wasm_bindgen]
impl WasmCadastre {
    #[wasm_bindgen]
    pub async fn from_ign_api(
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<WasmCadastre, JsValue> {
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode};

        if min_x >= max_x || min_y >= max_y {
            return Err(JsValue::from_str(
                "Invalid bounding box: min values must be less than max values",
            ));
        }

        let typename = "CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle";
        let base_url = "https://data.geopf.fr/wfs/ows";
        let bbox_str = format!("{},{},{},{}", min_y, min_x, max_y, max_x);

        let request_url = format!(
            "{}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES={}&OUTPUTFORMAT=application/json&Bbox={}&CRS=EPSG:4326&STARTINDEX=0&MAXFEATURES=10000",
            base_url, typename, bbox_str
        );

        let mut opts = RequestInit::new();
        opts.set_method("GET");
        opts.set_mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&request_url, &opts)
            .map_err(|e| JsValue::from_str(&format!("Failed to create request: {:?}", e)))?;

        let window =
            web_sys::window().ok_or_else(|| JsValue::from_str("No window object available"))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| JsValue::from_str(&format!("Network request failed: {:?}", e)))?;

        let resp: web_sys::Response = resp_value
            .dyn_into()
            .map_err(|_| JsValue::from_str("Invalid response type"))?;

        if !resp.ok() {
            return Err(JsValue::from_str(&format!(
                "IGN API error {}: {}",
                resp.status(),
                resp.status_text()
            )));
        }

        let text_promise = resp
            .text()
            .map_err(|e| JsValue::from_str(&format!("Failed to get response text: {:?}", e)))?;

        let text = JsFuture::from(text_promise)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to read response: {:?}", e)))?;

        let geojson_str = text
            .as_string()
            .ok_or_else(|| JsValue::from_str("Response is not a valid string"))?;

        Self::from_geojson(&geojson_str)
    }

    #[wasm_bindgen]
    pub fn from_geojson(geojson_str: &str) -> Result<WasmCadastre, JsValue> {
        let geojson: GeoJson = geojson_str
            .parse()
            .map_err(|e| JsValue::from_str(&format!("Invalid GeoJSON: {}", e)))?;

        match &geojson {
            GeoJson::FeatureCollection(_) | GeoJson::Feature(_) => Ok(WasmCadastre { geojson }),
            _ => Err(JsValue::from_str(
                "GeoJSON must be a Feature or FeatureCollection",
            )),
        }
    }

    #[wasm_bindgen]
    pub fn to_geojson(&self) -> Result<String, JsValue> {
        let fc = self.as_feature_collection()?;
        serde_json::to_string(&fc)
            .map_err(|e| JsValue::from_str(&format!("Serialization failed: {}", e)))
    }

    #[wasm_bindgen]
    pub fn len(&self) -> usize {
        Self::feature_count(&self.geojson)
    }

    #[wasm_bindgen]
    pub fn get_stats(&self) -> Result<JsValue, JsValue> {
        let stats = CadastreStats {
            count: Self::feature_count(&self.geojson),
            total_contenance: Self::sum_contenance(&self.geojson),
        };
        serde_wasm_bindgen::to_value(&stats)
            .map_err(|e| JsValue::from_str(&format!("Serialization failed: {}", e)))
    }

    fn as_feature_collection(&self) -> Result<FeatureCollection, JsValue> {
        match &self.geojson {
            GeoJson::FeatureCollection(fc) => Ok(fc.clone()),
            GeoJson::Feature(f) => Ok(FeatureCollection {
                bbox: None,
                features: vec![f.clone()],
                foreign_members: None,
            }),
            _ => Err(JsValue::from_str("Invalid GeoJSON type")),
        }
    }

    fn feature_count(geojson: &GeoJson) -> usize {
        match geojson {
            GeoJson::FeatureCollection(fc) => fc.features.len(),
            GeoJson::Feature(_) => 1,
            _ => 0,
        }
    }

    fn sum_contenance(geojson: &GeoJson) -> f64 {
        let features: Vec<&Feature> = match geojson {
            GeoJson::FeatureCollection(fc) => fc.features.iter().collect(),
            GeoJson::Feature(f) => vec![f],
            _ => return 0.0,
        };
        features
            .iter()
            .filter_map(|f| f.properties.as_ref())
            .filter_map(|props| {
                props.get("contenance").and_then(|v| {
                    v.as_f64()
                        .or_else(|| v.as_i64().map(|i| i as f64))
                        .or_else(|| v.as_u64().map(|u| u as f64))
                })
            })
            .sum()
    }
}

/// Set panic hook for better error messages (alternative to init)
#[wasm_bindgen]
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

/// DEM (Digital Elevation Model) reader for WASM
///
/// Uses geotiff 0.1 or tiff crate to read GeoTIFF metadata.
/// Note: geotiff 0.1 only provides metadata (width, height), not pixel data access.
#[wasm_bindgen]
pub struct WasmDem {
    _tiff: Option<GeoTiff>, // Keep for potential future use (may be None if geotiff couldn't read)
    width: usize,
    height: usize,
    bytes: Vec<u8>, // Keep bytes for potential future pixel parsing
}

#[wasm_bindgen]
impl WasmDem {
    /// Load DEM from IGN API
    ///
    /// # Arguments
    /// * `min_x` - Minimum longitude (west)
    /// * `min_y` - Minimum latitude (south)
    /// * `max_x` - Maximum longitude (east)
    /// * `max_y` - Maximum latitude (north)
    ///
    /// # Returns
    /// A new WasmDem instance loaded from IGN API
    ///
    /// # Errors
    /// Returns JsValue error if the API request fails or data is invalid
    #[wasm_bindgen]
    pub async fn from_ign_api(
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<WasmDem, JsValue> {
        use wasm_bindgen_futures::JsFuture;
        use web_sys::{Request, RequestInit, RequestMode};

        // Validate coordinates
        if min_x >= max_x || min_y >= max_y {
            return Err(JsValue::from_str(
                "Invalid bounding box: min values must be less than max values",
            ));
        }

        // Build WMS request URL for IGN API (DEM layer)
        // Following the pattern from ign_collect.rs execute_wms
        let layer = "ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES";
        let base_url = "https://data.geopf.fr/wms-r";

        // Calculate width and height (e.g., 512x512 for reasonable resolution)
        let width = 512;
        let height = 512;

        // Format bbox for WMS 1.3.0 with EPSG:4326
        // IMPORTANT: For WMS 1.3.0 with EPSG:4326, the bbox order is inverted for DEM
        // Python: if key == "ortho" and version == "1.3.0" and crs == "EPSG:4326": Bbox_str = [ymin, xmin, ymax, xmax]
        // Format: ymin, xmin, ymax, xmax (latitude, longitude order)
        let bbox_str = format!("{},{},{},{}", min_x, min_y, max_x, max_y);

        let request_url = format!(
            "{}?LAYERS={}&FORMAT=image/geotiff&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&STYLES=&CRS=EPSG:4326&Bbox={}&WIDTH={}&HEIGHT={}",
            base_url, layer, bbox_str, width, height
        );

        // Create and configure fetch request
        let opts = {
            let mut init = RequestInit::new();
            init.set_method("GET");
            init.set_mode(RequestMode::Cors);
            init
        };

        let request = Request::new_with_str_and_init(&request_url, &opts)
            .map_err(|e| JsValue::from_str(&format!("Failed to create request: {:?}", e)))?;

        // Execute request
        let window =
            web_sys::window().ok_or_else(|| JsValue::from_str("No window object available"))?;

        let resp_value = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| JsValue::from_str(&format!("Network request failed: {:?}", e)))?;

        let resp: web_sys::Response = resp_value
            .dyn_into()
            .map_err(|_| JsValue::from_str("Invalid response type"))?;

        // Check response status
        if !resp.ok() {
            return Err(JsValue::from_str(&format!(
                "IGN API error {}: {}",
                resp.status(),
                resp.status_text()
            )));
        }

        // Get response as ArrayBuffer
        let array_buffer_promise = resp
            .array_buffer()
            .map_err(|e| JsValue::from_str(&format!("Failed to get array buffer: {:?}", e)))?;

        let array_buffer = JsFuture::from(array_buffer_promise)
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed to read array buffer: {:?}", e)))?;

        let array_buffer: js_sys::ArrayBuffer = array_buffer
            .dyn_into()
            .map_err(|_| JsValue::from_str("Invalid array buffer type"))?;

        let bytes = js_sys::Uint8Array::new(&array_buffer).to_vec();

        // Load from bytes
        Self::from_bytes(&bytes)
    }

    /// Load DEM from TIFF file bytes
    ///
    /// # Arguments
    /// * `bytes` - TIFF file as Uint8Array from JavaScript
    ///
    /// # Errors
    /// Returns JsValue error if TIFF parsing fails
    #[wasm_bindgen]
    pub fn from_bytes(bytes: &[u8]) -> Result<WasmDem, JsValue> {
        let bytes_vec = bytes.to_vec();
        let cursor = Cursor::new(bytes_vec.clone());

        // Try to read with geotiff first
        let (width, height, tiff) = match GeoTiff::read(cursor) {
            Ok(tiff) => {
                // Successfully read with geotiff
                (tiff.raster_width, tiff.raster_height, Some(tiff))
            }
            Err(_) => {
                // geotiff failed, try with tiff crate as fallback
                let cursor = Cursor::new(bytes_vec.clone());
                let mut decoder = tiff::decoder::Decoder::new(cursor).map_err(|e| {
                    JsValue::from_str(&format!(
                        "Failed to read TIFF (geotiff and tiff both failed): {}",
                        e
                    ))
                })?;

                let dimensions = decoder.dimensions().map_err(|e| {
                    JsValue::from_str(&format!("Failed to get TIFF dimensions: {}", e))
                })?;

                (dimensions.0 as usize, dimensions.1 as usize, None)
            }
        };

        Ok(WasmDem {
            _tiff: tiff, // May be None if geotiff couldn't read it
            width,
            height,
            bytes: bytes_vec,
        })
    }

    /// Get raster width
    #[wasm_bindgen]
    pub fn width(&self) -> u32 {
        self.width as u32
    }

    /// Get raster height
    #[wasm_bindgen]
    pub fn height(&self) -> u32 {
        self.height as u32
    }

    /// Get extent (bounding box) as [min_x, min_y, max_x, max_y]
    /// Note: geotiff 0.1 may not have get_extent, so we return a placeholder
    #[wasm_bindgen]
    pub fn get_extent(&self) -> Vec<f64> {
        // Try to get extent from geotransform if available
        // For now, return placeholder - actual implementation depends on geotiff API
        // In a real implementation, you would extract this from the geotransform matrix
        vec![0.0, 0.0, self.width as f64, self.height as f64]
    }

    /// Get value at pixel coordinates (x, y)
    ///
    /// # Arguments
    /// * `x` - Pixel x coordinate (0 to width-1)
    /// * `y` - Pixel y coordinate (0 to height-1)
    /// * `sample_index` - Sample index (usually 0 for single-band DEM)
    ///
    /// # Returns
    /// Elevation value at the specified coordinates, or NaN if out of bounds
    ///
    /// # Note
    /// geotiff 0.1 only provides metadata (width, height), not pixel data access.
    /// To read actual pixel values, you would need to:
    /// 1. Use a different crate (like `tiff` or `image`) to read raw TIFF data
    /// 2. Or use GDAL bindings (not WASM-compatible)
    /// 3. Or parse the TIFF file format manually
    #[wasm_bindgen]
    pub fn get_value_at(&self, x: u32, y: u32, _sample_index: u32) -> f64 {
        if x as usize >= self.width || y as usize >= self.height {
            return f64::NAN;
        }

        // geotiff 0.1 doesn't provide pixel data access methods
        // This is a limitation of the crate version
        // For now, return NaN - actual implementation would require parsing TIFF data manually
        // or using a different library
        f64::NAN
    }

    /// Get statistics (min, max, mean) for the DEM
    ///
    /// # Note
    /// geotiff 0.1 only provides metadata, not pixel data access.
    /// This returns placeholder values. For actual statistics, you would need
    /// to parse the TIFF pixel data manually or use a different library.
    #[wasm_bindgen]
    pub fn get_stats(&self) -> Result<JsValue, JsValue> {
        // geotiff 0.1 limitation: no pixel data access
        // Return placeholder stats based on dimensions only
        let stats = DemStats {
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            count: (self.width * self.height) as usize,
        };

        serde_wasm_bindgen::to_value(&stats)
            .map_err(|e| JsValue::from_str(&format!("Serialization failed: {}", e)))
    }

    /// Get elevation values as a flat array (row-major order)
    /// Useful for creating height maps or visualizations
    ///
    /// # Note
    /// geotiff 0.1 only provides metadata, not pixel data access.
    /// This returns an array of NaN values. For actual data, you would need
    /// to parse the TIFF pixel data manually or use a different library.
    #[wasm_bindgen]
    pub fn get_elevation_array(&self) -> Vec<f64> {
        // geotiff 0.1 limitation: no pixel data access
        // Return array of NaN values as placeholder
        vec![f64::NAN; (self.width * self.height)]
    }
}

/// DEM statistics structure
#[derive(Serialize, Deserialize)]
struct DemStats {
    min: f64,
    max: f64,
    mean: f64,
    count: usize,
}
