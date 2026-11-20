// Simple example: Using BuildingCollection
use anyhow::Result;
use geo::polygon;
use rsmdu::geometric::building::{Building, BuildingCollection};

fn main() -> Result<()> {
    // Create a new collection
    let mut collection = BuildingCollection::new_simple(Some("./output".to_string()));
    collection.set_default_storey_height(3.0);

    // Create a building with a polygon geometry
    let footprint = polygon![
        (x: -1.15, y: 46.18),
        (x: -1.14, y: 46.18),
        (x: -1.14, y: 46.19),
        (x: -1.15, y: 46.19),
        (x: -1.15, y: 46.18),
    ];

    // Create a building with height
    let building = Building::with_height(footprint, 12.5);
    collection.add_building(building);

    // Process heights (compute missing heights)
    collection.process_heights();

    // Display information
    println!("Number of buildings: {}", collection.len());
    println!("Mean height: {:.2} m", collection.calculate_mean_height());

    // Convert to Polars DataFrame
    let df = collection.to_polars_df()?;
    println!("\nPolars DataFrame:");
    println!("{}", df);

    Ok(())
}
