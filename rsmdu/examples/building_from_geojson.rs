// Example: Using BuildingCollection with GeoJSON
// This example shows how to load buildings from GeoJSON
use anyhow::Result;
use rsmdu::geometric::building::BuildingCollection;

fn main() -> Result<()> {
    println!("=== Example: Loading buildings from GeoJSON ===\n");

    // Example GeoJSON FeatureCollection with several buildings
    let geojson_data = r#"
    {
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {
                    "hauteur": 15.5,
                    "nombre_d_etages": 5,
                    "nom": "Building A"
                },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[
                        [-1.152, 46.181],
                        [-1.150, 46.181],
                        [-1.150, 46.183],
                        [-1.152, 46.183],
                        [-1.152, 46.181]
                    ]]
                }
            },
            {
                "type": "Feature",
                "properties": {
                    "hauteur": 20.0,
                    "nom": "Building B"
                },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[
                        [-1.149, 46.181],
                        [-1.147, 46.181],
                        [-1.147, 46.183],
                        [-1.149, 46.183],
                        [-1.149, 46.181]
                    ]]
                }
            },
            {
                "type": "Feature",
                "properties": {
                    "nombre_d_etages": 4,
                    "nom": "Building C"
                },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[
                        [-1.146, 46.181],
                        [-1.144, 46.181],
                        [-1.144, 46.183],
                        [-1.146, 46.183],
                        [-1.146, 46.181]
                    ]]
                }
            }
        ]
    }
    "#;

    // Load buildings from GeoJSON
    let mut collection = BuildingCollection::from_geojson(
        geojson_data.as_bytes(),
        Some("./output".to_string()),
        3.0,  // Default storey height (3 meters)
        None, // CRS (Coordinate Reference System) - None uses default
    )?;

    println!("Buildings loaded: {}", collection.len());
    println!("\nBuilding details:");
    for (idx, building) in collection.buildings().iter().enumerate() {
        println!("  Building {}:", idx + 1);
        println!("    - Height: {:?} m", building.height);
        println!("    - Number of storeys: {:?}", building.nombre_d_etages);
        println!("    - Area: {:.2} m²", building.area);
        println!(
            "    - Centroid: ({:.6}, {:.6})",
            building.centroid.x(),
            building.centroid.y()
        );
        if !building.metadata.is_empty() {
            println!("    - Metadata: {:?}", building.metadata);
        }
    }

    // Compute mean height before processing
    println!(
        "\nMean height (before processing): {:.2} m",
        collection.calculate_mean_height()
    );

    // Process heights (fill missing heights)
    collection.process_heights();

    println!("\nAfter height processing:");
    for (idx, building) in collection.buildings().iter().enumerate() {
        println!("  Building {}: height = {:?} m", idx + 1, building.height);
    }

    // Convert to Polars DataFrame
    println!("\nConverting to Polars DataFrame...");
    let df = collection.to_polars_df()?;
    println!(
        "DataFrame created with {} rows and {} columns",
        df.height(),
        df.width()
    );
    println!("\nDataFrame preview:");
    println!("{}", df);

    Ok(())
}
