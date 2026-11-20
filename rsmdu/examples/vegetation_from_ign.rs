use anyhow::Result;
use rsmdu::geometric::vegetation::Vegetation;

/// Example: Loading Vegetation data from IGN API (NDVI calculation)
/// Following Python example from pymdu.geometric.Vegetation
fn main() -> Result<()> {
    println!("=== Example: Vegetation from IGN API (NDVI) ===\n");

    // Create Vegetation instance
    // Python: vegetation = Vegetation(output_path='./', min_area=0)
    let mut vegetation = Vegetation::new(None, Some("./output".to_string()), None, false, 0.0)?;

    // Set bounding box (La Rochelle, France)
    // Python: vegetation.bbox = [-1.152704, 46.181627, -1.139893, 46.18699]
    vegetation.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);

    println!("Bounding box set:");
    println!("  - Longitude: -1.152704 to -1.139893");
    println!("  - Latitude: 46.181627 to 46.18699");
    println!("  - Area: La Rochelle, France");
    println!("  - Format: WGS84 (EPSG:4326)\n");

    // Run vegetation processing
    // Python: vegetation = vegetation.run()
    println!("Downloading IRC image and computing NDVI...");
    println!("  - Downloading from IGN API...");
    println!("  - NDVI = (NIR - Red) / (NIR + Red)...");
    println!("  - Filtering pixels with NDVI < 0.2...");
    println!("  - Polygonizing raster...");
    println!("  - Filtering polygons (NDVI == 0, area > min_area)...");

    let vegetation_result = vegetation.run()?;

    println!("\nVegetation processed successfully!");

    // Get GeoJSON (equivalent to to_gdf() in Python)
    if let Some(geojson) = vegetation_result.get_geojson() {
        match geojson {
            geojson::GeoJson::FeatureCollection(fc) => {
                println!("  - Number of vegetation polygons: {}", fc.features.len());
            }
            _ => {
                println!("  - GeoJSON loaded (non-FeatureCollection format)");
            }
        }
    }

    // Save to GeoJSON
    // Python: vegetation.to_geojson(name="vegetation")
    println!("\nSaving to GeoJSON...");
    vegetation_result.to_geojson(None)?;

    println!("\n✅ Processing complete!");
    println!("  - Output file: {:?}", vegetation_result.get_output_path());

    Ok(())
}
