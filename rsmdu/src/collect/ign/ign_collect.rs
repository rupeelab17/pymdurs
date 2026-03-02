use anyhow::{Context, Result};
use csv::ReaderBuilder;
use encoding_rs;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{create_dir_all, write, File};
use std::io::{BufReader, Read};
use std::path::PathBuf;

use crate::collect::global_variables::TEMP_PATH;
use crate::geo_core::{BoundingBox, GeoCore};

/// Official IGN URL for the services table CSV (fallback when no local file is found).
const IGN_CSV_URL: &str =
    "https://geoservices.ign.fr/sites/default/files/2026-02/Tableau-suivi-services-web-06-02-2026.csv";

const CSV_NAME: &str = "Tableau-suivi-services-web-06-02-2026.csv";

/// CSV row structure for IGN services
#[derive(Debug, Clone)]
pub struct IgnServiceRow {
    pub service: String,
    pub nom_technique: String,
    pub url_geoplateforme: String,
}

/// Row deserialized from IGN CSV (headers: Service;Nom technique;URL d'acces Geoplateforme;...)
#[derive(Debug, Deserialize)]
struct IgnCsvRecord {
    #[serde(rename = "Service")]
    service: String,
    #[serde(rename = "Nom technique")]
    nom_technique: String,
    #[serde(rename = "URL d'acces Geoplateforme")]
    url_geoplateforme: String,
}

/// Base struct for IGN data collection
/// Provides methods to query IGN API and fetch geospatial data
/// Follows the Python implementation from pymdu
pub struct IgnCollect {
    pub content: Option<Vec<u8>>,
    pub bbox: Option<BoundingBox>,
    pub filter_xml: Option<String>,
    pub cql_filter: Option<String>,
    pub ign_keys: HashMap<String, String>,
    pub geo_core: GeoCore,
    pub df_csv_file: HashMap<String, IgnServiceRow>, // Indexed by nom_technique
    #[allow(dead_code)] // Reserved for future use
    collect_path: PathBuf,
}

impl IgnCollect {
    pub fn new() -> Result<Self> {
        let mut ign_keys = HashMap::new();
        ign_keys.insert("buildings".to_string(), "BDTOPO_V3:batiment".to_string());
        ign_keys.insert("cosia".to_string(), "IGNF_COSIA_2024-2026".to_string());
        ign_keys.insert("water".to_string(), "BDTOPO_V3:plan_d_eau".to_string());
        ign_keys.insert("road".to_string(), "BDTOPO_V3:troncon_de_route".to_string());
        ign_keys.insert(
            "irc".to_string(),
            "ORTHOIMAGERY.ORTHOPHOTOS.IRC".to_string(),
        );
        ign_keys.insert(
            "ortho".to_string(),
            "HR.ORTHOIMAGERY.ORTHOPHOTOS".to_string(),
        );
        ign_keys.insert(
            "dem".to_string(),
            "ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES".to_string(),
        );
        ign_keys.insert(
            "dsm".to_string(),
            "ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES.MNS".to_string(),
        );
        ign_keys.insert(
            "cadastre".to_string(),
            "CADASTRALPARCELS.PARCELLAIRE_EXPRESS:parcelle".to_string(),
        );
        ign_keys.insert(
            "iris".to_string(),
            "STATISTICALUNITS.IRIS:contours_iris".to_string(),
        );
        ign_keys.insert(
            "hydrographique".to_string(),
            "BDCARTO_V5:detail_hydrographique".to_string(),
        );
        ign_keys.insert(
            "vegetation".to_string(),
            "BDTOPO_V3:zone_de_vegetation".to_string(),
        );
        ign_keys.insert("isochrone".to_string(), "bdtopo-valhalla".to_string());
        ign_keys.insert(
            "altitude".to_string(),
            "SERVICE_CALCUL_ALTIMETRIQUE".to_string(),
        );

        // Load CSV file - try to find it in multiple locations
        let csv_path = Self::find_csv_file()?;
        let collect_path = csv_path
            .parent()
            .unwrap_or(&PathBuf::from("."))
            .to_path_buf();

        let df_csv_file = Self::load_csv_file(&csv_path)?;

        Ok(IgnCollect {
            content: None,
            bbox: None,
            filter_xml: None,
            cql_filter: None,
            ign_keys,
            geo_core: GeoCore::default(),
            df_csv_file,
            collect_path, // Store path for potential future use
        })
    }

    /// Find CSV file in multiple possible locations, or download from IGN URL and cache in TEMP_PATH.
    fn find_csv_file() -> Result<PathBuf> {
        // 1) Dev: CARGO_MANIFEST_DIR
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let csv_path = PathBuf::from(manifest_dir)
            .join("src/collect/ign/data")
            .join(CSV_NAME);
        if csv_path.exists() {
            return Ok(csv_path);
        }

        // 2) Relative paths from cwd
        for path in &[
            PathBuf::from("src/collect/ign/data").join(CSV_NAME),
            PathBuf::from("./src/collect/ign/data").join(CSV_NAME),
        ] {
            if path.exists() {
                return Ok(path.clone());
            }
        }

        // 3) Cache in TEMP_PATH (from a previous download)
        let cache_path = PathBuf::from(TEMP_PATH).join(CSV_NAME);
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // 4) Download from IGN and save to TEMP_PATH
        let client = Client::new();
        let response = client
            .get(IGN_CSV_URL)
            .send()
            .with_context(|| format!("Failed to download IGN CSV from {}", IGN_CSV_URL))?;
        if !response.status().is_success() {
            anyhow::bail!(
                "IGN CSV URL returned {}: {}",
                response.status(),
                IGN_CSV_URL
            );
        }
        let bytes = response
            .bytes()
            .with_context(|| format!("Failed to read response body from {}", IGN_CSV_URL))?;
        create_dir_all(TEMP_PATH).context("Failed to create temp directory for IGN CSV")?;
        write(&cache_path, &bytes)
            .with_context(|| format!("Failed to write IGN CSV to {}", cache_path.display()))?;
        Ok(cache_path)
    }

    /// Load CSV file with the `csv` crate and index by nom_technique.
    /// Uses semicolon delimiter and ISO-8859-1 encoding (IGN table format).
    fn load_csv_file(csv_path: &PathBuf) -> Result<HashMap<String, IgnServiceRow>> {
        let file =
            File::open(csv_path).context(format!("Failed to open CSV file: {:?}", csv_path))?;

        let mut buffer = Vec::new();
        BufReader::new(file).read_to_end(&mut buffer)?;

        let encoding =
            encoding_rs::Encoding::for_label(b"ISO-8859-1").unwrap_or(encoding_rs::WINDOWS_1252);
        let (decoded, _, _) = encoding.decode(&buffer);
        let decoded_str: &str = decoded.as_ref();

        let mut rdr = ReaderBuilder::new()
            .delimiter(b';')
            .has_headers(true)
            .from_reader(decoded_str.as_bytes());

        let mut df_csv_file = HashMap::new();

        for result in rdr.deserialize() {
            let record: IgnCsvRecord = result.context("Failed to deserialize CSV record")?;
            if record.nom_technique.is_empty() {
                continue;
            }
            df_csv_file.insert(
                record.nom_technique.clone(),
                IgnServiceRow {
                    service: record.service,
                    nom_technique: record.nom_technique.clone(),
                    url_geoplateforme: record.url_geoplateforme,
                },
            );
        }

        Ok(df_csv_file)
    }

    /// Get row from CSV by key
    pub fn get_row_ressource(&self, key: &str) -> Option<&IgnServiceRow> {
        let typename = self.ign_keys.get(key)?;
        self.df_csv_file.get(typename)
    }

    /// Execute IGN API request following Python implementation
    ///
    /// This method uses the Géoplateforme URLs from the CSV file to make requests.
    /// According to the Géoplateforme documentation:
    /// - WFS services: https://data.geopf.fr/wfs/ows (Service Géoplateforme de sélection WFS)
    /// - WMS-Raster: https://data.geopf.fr/wms-r/wms (Service Géoplateforme d'images WMS-Raster)
    /// - WMS-Vecteur: https://data.geopf.fr/wms-v/wms (Service Géoplateforme d'images WMS-Vecteur)
    /// - WMTS: https://data.geopf.fr/wmts (Service Géoplateforme d'images tuilées WMTS)
    ///
    /// The service follows OGC WFS 2.0.0 and WMS 1.3.0 standards.
    /// See: https://geoservices.ign.fr/documentation/services/services-geoplateforme/diffusion
    pub fn execute_ign(&mut self, key: &str) -> Result<()> {
        let typename = self
            .ign_keys
            .get(key)
            .context(format!("Unknown IGN key: {}", key))?
            .clone();

        let bbox = self
            .bbox
            .context("Bounding box must be set before executing IGN request")?;

        println!("Bbox: {:?}", bbox);

        // Get URL from CSV following Python implementation
        // Python: row = self.df_csv_file.loc[(self.df_csv_file.index == self.ign_keys[key])]
        //         url = row["URL d'acces Geoplateforme"].values[0].split("&REQUEST=GetCapabilities")[0]
        let row = self
            .get_row_ressource(key)
            .context(format!("No CSV row found for key: {}", key))?;

        println!("Row: {:?}", row);

        // Extract base URL from Géoplateforme URL (remove GetCapabilities part)
        // Following Python: url.split("&REQUEST=GetCapabilities")[0]
        // This gives us the base URL like: https://data.geopf.fr/wfs/ows
        let url = if row.url_geoplateforme.contains("&REQUEST=GetCapabilities") {
            row.url_geoplateforme
                .split("&REQUEST=GetCapabilities")
                .next()
                .unwrap_or(&row.url_geoplateforme)
                .to_string()
        } else if row.url_geoplateforme.contains("?SERVICE=") {
            // If URL already has parameters, extract base URL
            row.url_geoplateforme
                .split('?')
                .next()
                .unwrap_or(&row.url_geoplateforme)
                .to_string()
        } else {
            // If no parameters, use URL as-is
            row.url_geoplateforme.clone()
        };

        println!("URL: {}", url);

        let client = Client::new();

        // Handle different service types based on key
        if matches!(
            key,
            "buildings" | "road" | "water" | "cadastre" | "iris" | "vegetation" | "hydrographique"
        ) {
            // WFS request
            self.execute_wfs(&client, &url, &typename, &bbox, key)?;
        } else if key == "isochrone" {
            // Isochrone request (POST with JSON)
            anyhow::bail!("Isochrone requests require additional parameters (resource, costValue, point) - not yet implemented");
        } else {
            // WMS request (for ortho, dem, cosia, etc.)
            self.execute_wms(&client, &url, &typename, &bbox, key)?;
        }

        Ok(())
    }

    /// Execute WFS request following Python implementation
    /// Uses owslib.wfs.WebFeatureService.getfeature() equivalent
    fn execute_wfs(
        &mut self,
        client: &Client,
        url: &str,
        typename: &str,
        bbox: &BoundingBox,
        _key: &str,
    ) -> Result<()> {
        // Build filter XML if CQL filter is set (following Python logic)
        // Python: if self._cql_filter: Bbox = Bbox(Bbox=self._Bbox, crs="EPSG:4326")
        let filter_xml = if self.cql_filter.is_some() {
            // For CQL filter, build Bbox filter XML
            Some(self.build_bbox_filter_xml(bbox)?)
        } else {
            None
        };

        // Python: if filter_xml is set, self._Bbox = None (Bbox is in filter)
        let use_bbox_in_url = filter_xml.is_none();

        // Build WFS GetFeature request following Python implementation
        // Python: wfs2.getfeature(typename=typename, Bbox=self._Bbox, filter=self.filter_xml,
        //                          startindex=0, maxfeatures=10000, outputFormat="application/json")
        //
        // According to Géoplateforme documentation:
        // - WFS service: https://data.geopf.fr/wfs/ows (Service Géoplateforme de sélection WFS)
        // - Uses OGC WFS 2.0.0 standard
        // URL base from CSV should already have ?SERVICE=WFS&VERSION=2.0.0
        let mut request_url = if url.contains('?') {
            format!(
                "{}&REQUEST=GetFeature&TYPENAMES={}&OUTPUTFORMAT=application/json",
                url, typename
            )
        } else {
            format!("{}?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES={}&OUTPUTFORMAT=application/json", url, typename)
        };

        // Add Bbox if not in filter (following Python: Bbox=self._Bbox)
        if use_bbox_in_url {
            request_url.push_str(&format!(
                "&Bbox={},{},{},{}&CRS=EPSG:4326",
                bbox.min_y, bbox.min_x, bbox.max_y, bbox.max_x,
            ));
        }

        // Add filter if present (Python: filter=self.filter_xml)
        if let Some(ref filter_xml) = filter_xml {
            request_url.push_str(&format!("&FILTER={}", urlencoding::encode(filter_xml)));
        }

        // Store filter for potential future use
        self.filter_xml = filter_xml.clone();

        // Add maxfeatures and startindex (following Python: startindex=0, maxfeatures=10000)
        request_url.push_str("&STARTINDEX=0&MAXFEATURES=10000");

        println!("Request URL WFS: {}", request_url);

        let response = client
            .get(&request_url)
            .send()
            .context("Failed to send WFS request to IGN API")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            anyhow::bail!("IGN API returned error {}: {}", status, body);
        }

        let content_bytes = response
            .bytes()
            .context("Failed to read response body")?
            .to_vec();

        self.content = Some(content_bytes);
        Ok(())
    }

    /// Execute WMS request following Python implementation
    fn execute_wms(
        &mut self,
        client: &Client,
        url: &str,
        typename: &str,
        bbox: &BoundingBox,
        key: &str,
    ) -> Result<()> {
        // For WMS, calculate image dimensions based on resolution
        // Python uses: resolution = kwargs.get("resolution") or 1.0
        let resolution = 1.0; // Default resolution in meters/pixel

        // Calculate center (Python: lon_center = (xmin + xmax) / 2)
        // let _lon_center = (bbox.min_x + bbox.max_x) / 2.0;
        let lat_center = (bbox.min_y + bbox.max_y) / 2.0;

        // Conversion deg → m (approximate, valid near France)
        // Python: deg_to_m_lat = 111320, deg_to_m_lon = 40075000 * cos(radians(lat_center)) / 360
        use std::f64::consts::PI;
        let deg_to_m_lat = 111320.0;
        let deg_to_m_lon = 40075000.0 * (lat_center * PI / 180.0).cos() / 360.0;

        let width_m = (bbox.max_x - bbox.min_x) * deg_to_m_lon;
        let height_m = (bbox.max_y - bbox.min_y) * deg_to_m_lat;

        let width_px = (width_m / resolution) as u32;
        let height_px = (height_m / resolution) as u32;

        // For WMS 1.3.0 with EPSG:4326, Bbox order is inverted for ortho and dem
        // Python: if key == "ortho" and version == "1.3.0" and crs == "EPSG:4326": Bbox_str = [ymin, xmin, ymax, xmax]
        // Python for dem: "Bbox": f"{self._Bbox[1]},{self._Bbox[0]},{self._Bbox[3]},{self._Bbox[2]}"
        // This means: [ymin, xmin, ymax, xmax]
        let bbox_str = if matches!(key, "ortho" | "dem" | "cosia") {
            format!(
                "{},{},{},{}",
                bbox.min_y, bbox.min_x, bbox.max_y, bbox.max_x
            )
        } else {
            format!(
                "{},{},{},{}",
                bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y
            )
        };

        // Build WMS GetMap request following Python implementation
        // Python: wms.getmap(layers=[typename], srs=crs, crs=crs, Bbox=Bbox_str,
        //                    size=(width_px, height_px), exceptions="text/xml",
        //                    format="image/geotiff", transparent=True, styles=["normal"])
        //
        // According to Géoplateforme documentation:
        // - WMS-Raster: https://data.geopf.fr/wms-r (Service Géoplateforme d'images WMS-Raster)
        // - WMS-Vecteur: https://data.geopf.fr/wms-v/wms (Service Géoplateforme d'images WMS-Vecteur)
        // Both use WMS 1.3.0 standard
        //
        // For DEM and raster services, use wms-r endpoint:
        // https://data.geopf.fr/wms-r?LAYERS={couche}&FORMAT={format}&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&STYLES=&CRS={crs}&Bbox={Xmin,Ymin,Xmax,Ymax}&WIDTH={largeur}&HEIGHT={hauteur}

        // Build GetMap request according to OGC WMS 1.3.0 specification
        // Required parameters: SERVICE, VERSION, REQUEST, LAYERS, CRS, Bbox, WIDTH, HEIGHT, FORMAT
        // Optional: STYLES, TRANSPARENT, EXCEPTIONS
        let request_url = if matches!(key, "dem" | "irc" | "cosia" | "dsm") {
            // For DEM and other raster services, use wms-r endpoint with exact format
            // Format: LAYERS={couche}&FORMAT={format}&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&STYLES=&CRS={crs}&Bbox={Xmin,Ymin,Xmax,Ymax}&WIDTH={largeur}&HEIGHT={hauteur}
            format!(
                "https://data.geopf.fr/wms-r/wms?LAYERS={}&FORMAT=image/geotiff&SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&STYLES=&CRS=EPSG:4326&Bbox={}&WIDTH={}&HEIGHT={}",
                typename, bbox_str, width_px, height_px
            )
        } else {
            // For other services, use URL from CSV with original format
            let base_url = if url.contains('?') {
                format!("{}&", url)
            } else {
                format!("{}?", url)
            };
            format!(
                "{}SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS={}&CRS=EPSG:4326&Bbox={}&WIDTH={}&HEIGHT={}&FORMAT=image/geotiff&TRANSPARENT=true&STYLES=normal&EXCEPTIONS=text/xml",
                base_url, typename, bbox_str, width_px, height_px
            )
        };

        println!("Request URL WMS: {}", request_url);

        let response = client
            .get(&request_url)
            .send()
            .context("Failed to send WMS request to IGN API")?;

        if !response.status().is_success() {
            anyhow::bail!("IGN API returned error: {}", response.status());
        }

        let content_bytes = response
            .bytes()
            .context("Failed to read response body")?
            .to_vec();

        // For some keys, save to file and validate as GeoTIFF
        if matches!(key, "irc" | "dem" | "cosia") {
            let output_path = PathBuf::from(TEMP_PATH).join(format!("{}.tiff", key));

            // Create directory if it doesn't exist
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent)
                    .context(format!("Failed to create directory: {:?}", parent))?;
            }

            // Write file first
            std::fs::write(&output_path, &content_bytes)
                .context(format!("Failed to write file: {:?}", output_path))?;

            // Validate that it's a valid GeoTIFF using GDAL (more reliable than geotiff crate)
            // GDAL can read a wider variety of GeoTIFF formats
            match gdal::Dataset::open(&output_path) {
                Ok(_dataset) => {
                    println!("Saved and validated GeoTIFF: {:?}", output_path);
                }
                Err(e) => {
                    eprintln!("Warning: File saved but GeoTIFF validation failed: {}", e);
                    eprintln!("  File may still be valid but GDAL couldn't read it");
                }
            }
        }

        self.content = Some(content_bytes);
        Ok(())
    }

    /// Build Bbox filter XML following Python implementation
    /// Python: Bbox = Bbox(Bbox=self._Bbox, crs="EPSG:4326")
    ///         self.filter_xml = ElementTree.tostring(Bbox.toXML(), encoding="ascii", method="xml", xml_declaration=True).decode("utf-8")
    fn build_bbox_filter_xml(&self, bbox: &BoundingBox) -> Result<String> {
        // Build XML filter for Bbox following OGC Filter Encoding (OGC FES 2.0)
        // Python uses owslib.fes2.Bbox which generates XML like:
        // <ogc:Bbox xmlns:ogc="http://www.opengis.net/ogc">
        //   <ogc:PropertyName>Geometry</ogc:PropertyName>
        //   <gml:Envelope srsName="EPSG:4326">
        //     <gml:lowerCorner>min_y min_x</gml:lowerCorner>
        //     <gml:upperCorner>max_y max_x</gml:upperCorner>
        //   </gml:Envelope>
        // </ogc:Bbox>
        let filter_xml = format!(
            r#"<ogc:Filter xmlns:ogc="http://www.opengis.net/ogc" xmlns:gml="http://www.opengis.net/gml">
    <ogc:Bbox>
        <ogc:PropertyName>Geometry</ogc:PropertyName>
        <gml:Envelope srsName="EPSG:4326">
            <gml:lowerCorner>{} {}</gml:lowerCorner>
            <gml:upperCorner>{} {}</gml:upperCorner>
        </gml:Envelope>
    </ogc:Bbox>
</ogc:Filter>"#,
            bbox.min_y, bbox.min_x, bbox.max_y, bbox.max_x
        );
        Ok(filter_xml)
    }

    /// Set CQL filter
    pub fn set_cql_filter(&mut self, cql_filter: Option<String>) {
        self.cql_filter = cql_filter;
    }

    /// Fetch GeoJSON data from IGN API (returns bytes)
    pub fn fetch_geojson(&mut self, key: &str) -> Result<()> {
        self.execute_ign(key)?;
        Ok(())
    }

    /// Set bounding box from coordinates
    pub fn set_bbox(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) {
        self.bbox = Some(BoundingBox::new(min_x, min_y, max_x, max_y));
    }

    /// Get the content as a string (for debugging)
    pub fn content_as_string(&self) -> Result<String> {
        let content = self.content.as_ref().context("No content available")?;
        String::from_utf8(content.clone()).context("Content is not valid UTF-8")
    }
}

// Note: Cannot implement Default because new() returns Result
// Use IgnCollect::new()? instead

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ign_collect_new() {
        let ign = IgnCollect::new().unwrap();
        assert!(ign.ign_keys.contains_key("buildings"));
    }

    #[test]
    fn test_set_Bbox() {
        let mut ign = IgnCollect::new().unwrap();
        ign.set_bbox(-1.0, 46.0, -0.9, 46.1);
        assert!(ign.bbox.is_some());
        let Bbox = ign.bbox.unwrap();
        assert_eq!(Bbox.min_x, -1.0);
        assert_eq!(Bbox.min_y, 46.0);
        assert_eq!(Bbox.max_x, -0.9);
        assert_eq!(Bbox.max_y, 46.1);
    }

    #[test]
    fn test_ign_keys() {
        let ign = IgnCollect::new().unwrap();
        assert_eq!(
            ign.ign_keys.get("buildings"),
            Some(&"BDTOPO_V3:batiment".to_string())
        );
        assert_eq!(
            ign.ign_keys.get("water"),
            Some(&"BDTOPO_V3:plan_d_eau".to_string())
        );
    }

    #[test]
    fn test_get_row_ressource() {
        let ign = IgnCollect::new().unwrap();
        let row = ign.get_row_ressource("buildings");
        // This will be Some if the CSV contains the entry
        if let Some(row) = row {
            assert!(!row.url_geoplateforme.is_empty());
        }
    }
}
