use anyhow::Result;
use rsmdu::geometric::water::Water;

/// Example: Loading Water (plan d'eau) data from IGN API
/// Following Python example from pymdu.geometric.Water
fn main() -> Result<()> {
    println!("=== Example: Loading Water from IGN API ===\n");

    // Create Water instance
    // Python: water = Water(output_path='./')
    let mut water = Water::new(None, Some("./output".to_string()), None)?;

    // Set bounding box (La Rochelle, France)
    // Python: water.bbox = [-1.152704, 46.181627, -1.139893, 46.18699]
    water.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);

    println!("Bounding box set:");
    println!("  - Longitude: -1.152704 to -1.139893");
    println!("  - Latitude: 46.181627 to 46.18699");
    println!("  - Area: La Rochelle, France");
    println!("  - Format: WGS84 (EPSG:4326)\n");

    // Run water processing
    // Python: water = water.run()
    println!("Downloading and processing Water from IGN API...");
    let water_result = water.run()?;

    println!("\nWater processed successfully!");

    // Get GeoJSON (equivalent to to_gdf() in Python)
    if let Some(geojson) = water_result.get_geojson() {
        match geojson {
            geojson::GeoJson::FeatureCollection(fc) => {
                println!("  - Number of water bodies: {}", fc.features.len());
            }
            _ => {
                println!("  - GeoJSON loaded (non-FeatureCollection format)");
            }
        }
    }

    // Save to GeoJSON
    // Python: water.to_geojson(name="water")
    println!("\nSaving to GeoJSON...");
    water_result.to_geojson(None)?;

    println!("\n✅ Processing complete!");
    println!("  - Output file: {:?}", water_result.get_output_path());

    Ok(())
}
