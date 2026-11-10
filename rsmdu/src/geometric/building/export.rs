//! Export buildings to Polars / GeoJSON.

use anyhow::Result;
#[cfg(feature = "polars")]
use anyhow::Context;
use geojson::{GeoJson, Geometry};
use std::path::Path;

#[cfg(feature = "polars")]
use polars::prelude::df;

use super::types::{Building, BuildingCollection};

impl BuildingCollection {
    /// Convert building collection to Polars DataFrame
    /// Similar to to_gdf() in Python version
    /// Returns a standard Polars DataFrame (not GeoDataFrame)
    #[cfg(feature = "polars")]
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
    #[cfg(feature = "polars")]
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
            let geometry: Geometry = (&geo_geom).into();

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
