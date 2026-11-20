use anyhow::Result;
use rsmdu::geometric::cadastre::Cadastre;

/// Example: Loading Cadastre (parcel) data from IGN API
/// Following Python example from pymdu.geometric.Cadastre
fn main() -> Result<()> {
    println!("=== Example: Loading Cadastre from IGN API ===\n");

    // Create Cadastre instance
    // Python: cadastre = Cadastre(output_path='./')
    let mut cadastre = Cadastre::new(Some("./output".to_string()))?;

    // Set bounding box (La Rochelle, France)
    // Python: cadastre.bbox = [-1.152704, 46.181627, -1.139893, 46.18699]
    cadastre.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);

    println!("Bounding box set:");
    println!("  - Longitude: -1.152704 to -1.139893");
    println!("  - Latitude: 46.181627 to 46.18699");
    println!("  - Area: La Rochelle, France");
    println!("  - Format: WGS84 (EPSG:4326)\n");

    // Run cadastre processing
    // Python: cadastre = cadastre.run()
    println!("Downloading and processing Cadastre from IGN API...");
    let cadastre_result = cadastre.run()?;

    println!("\nCadastre processed successfully!");

    // Get GeoJSON (equivalent to to_gdf() in Python)
    if let Some(geojson) = cadastre_result.get_geojson() {
        match geojson {
            geojson::GeoJson::FeatureCollection(fc) => {
                println!("  - Number of parcels: {}", fc.features.len());
            }
            _ => {
                println!("  - GeoJSON loaded (non-FeatureCollection format)");
            }
        }
    }

    // Save to GeoJSON
    // Python: cadastre.to_geojson(name="cadastre")
    println!("\nSaving to GeoJSON...");
    cadastre_result.to_geojson(None)?;

    println!("\n✅ Processing complete!");
    println!("  - Output file: {:?}", cadastre_result.get_output_path());

    Ok(())
}
