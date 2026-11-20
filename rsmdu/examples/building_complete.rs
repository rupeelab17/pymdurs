use anyhow::Result;
use rsmdu::geometric::building::BuildingCollection;

fn main() -> Result<()> {
    println!("=== BuildingCollection usage example ===\n");

    // Example 1: Create empty collection and add buildings manually
    println!("1. Creating empty collection and adding buildings:");
    example_manual_buildings()?;

    // Example 2: Load from GeoJSON
    println!("\n2. Loading from GeoJSON:");
    example_from_geojson()?;

    // Example 3: Height processing
    println!("\n3. Height processing:");
    example_process_heights()?;

    // Example 4: Conversion to Polars DataFrame
    println!("\n4. Conversion to Polars DataFrame:");
    example_to_polars()?;

    Ok(())
}

/// Example 1: Create buildings manually
fn example_manual_buildings() -> Result<()> {
    use geo::polygon;
    use rsmdu::geometric::building::Building;

    let mut collection = BuildingCollection::new_simple(None);

    // Create first building with height
    let poly1 = polygon![
        (x: 0.0, y: 0.0),
        (x: 10.0, y: 0.0),
        (x: 10.0, y: 10.0),
        (x: 0.0, y: 10.0),
        (x: 0.0, y: 0.0),
    ];
    let building1 = Building::with_height(poly1, 15.0);
    collection.add_building(building1);

    // Create second building without height but with number of storeys
    let poly2 = polygon![
        (x: 15.0, y: 0.0),
        (x: 25.0, y: 0.0),
        (x: 25.0, y: 10.0),
        (x: 15.0, y: 10.0),
        (x: 15.0, y: 0.0),
    ];
    let mut building2 = Building::new(poly2);
    building2.set_nombre_d_etages(5.0);
    collection.add_building(building2);

    println!("  - Number of buildings: {}", collection.len());
    println!(
        "  - Mean neighbourhood height: {:.2} m",
        collection.calculate_mean_height()
    );

    Ok(())
}

/// Example 2: Load from GeoJSON
fn example_from_geojson() -> Result<()> {
    // Example GeoJSON FeatureCollection
    let geojson_data = r#"
    {
        "type": "FeatureCollection",
        "features": [
            {
                "type": "Feature",
                "properties": {
                    "hauteur": 12.5,
                    "nombre_d_etages": 4
                },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]]
                }
            },
            {
                "type": "Feature",
                "properties": {
                    "hauteur": 20.0
                },
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[15, 0], [25, 0], [25, 10], [15, 10], [15, 0]]]
                }
            }
        ]
    }
    "#;

    let collection = BuildingCollection::from_geojson(
        geojson_data.as_bytes(),
        None,
        3.0,  // default_storey_height
        None, // set_crs
    )?;

    println!("  - Number of buildings loaded: {}", collection.len());

    for (idx, building) in collection.buildings().iter().enumerate() {
        println!(
            "  - Building {}: height={:?}, area={:.2} m²",
            idx + 1,
            building.height,
            building.area
        );
    }

    Ok(())
}

/// Example 3: Height processing
fn example_process_heights() -> Result<()> {
    use geo::polygon;
    use rsmdu::geometric::building::Building;

    let mut collection = BuildingCollection::new_simple(None);
    collection.set_default_storey_height(3.0);

    // Building 1: with height
    let poly1 = polygon![
        (x: 0.0, y: 0.0),
        (x: 10.0, y: 0.0),
        (x: 10.0, y: 10.0),
        (x: 0.0, y: 10.0),
        (x: 0.0, y: 0.0),
    ];
    let building1 = Building::with_height(poly1, 12.0);
    collection.add_building(building1);

    // Building 2: without height but with number of storeys
    let poly2 = polygon![
        (x: 15.0, y: 0.0),
        (x: 25.0, y: 0.0),
        (x: 25.0, y: 10.0),
        (x: 15.0, y: 10.0),
        (x: 15.0, y: 0.0),
    ];
    let mut building2 = Building::new(poly2);
    building2.set_nombre_d_etages(5.0);
    collection.add_building(building2);

    // Building 3: without height or storeys (will use mean height)
    let poly3 = polygon![
        (x: 30.0, y: 0.0),
        (x: 40.0, y: 0.0),
        (x: 40.0, y: 10.0),
        (x: 30.0, y: 10.0),
        (x: 30.0, y: 0.0),
    ];
    let building3 = Building::new(poly3);
    collection.add_building(building3);

    println!("  - Before processing:");
    println!(
        "    * Building 1: height={:?}",
        collection.buildings()[0].height
    );
    println!(
        "    * Building 2: height={:?}, storeys={:?}",
        collection.buildings()[1].height,
        collection.buildings()[1].nombre_d_etages
    );
    println!(
        "    * Building 3: height={:?}",
        collection.buildings()[2].height
    );
    println!(
        "    * Mean height: {:.2} m",
        collection.calculate_mean_height()
    );

    // Process heights
    collection.process_heights();

    println!("  - After processing:");
    for (idx, building) in collection.buildings().iter().enumerate() {
        println!("    * Building {}: height={:?} m", idx + 1, building.height);
    }

    Ok(())
}

/// Example 4: Conversion to Polars DataFrame
fn example_to_polars() -> Result<()> {
    use geo::polygon;
    use rsmdu::geometric::building::Building;

    let mut collection = BuildingCollection::new_simple(None);

    // Add a few buildings
    for i in 0..3 {
        let x = (i * 15) as f64;
        let poly = polygon![
            (x: x, y: 0.0),
            (x: x + 10.0, y: 0.0),
            (x: x + 10.0, y: 10.0),
            (x: x, y: 10.0),
            (x: x, y: 0.0),
        ];
        let building = Building::with_height(poly, 10.0 + i as f64 * 5.0);
        collection.add_building(building);
    }

    // Convert to Polars DataFrame
    let df = collection.to_polars_df()?;

    println!("  - DataFrame created with {} rows", df.height());
    println!("  - Columns: {:?}", df.get_column_names());

    // Display some statistics
    if let Ok(hauteur_col) = df.column("hauteur") {
        println!("  - Column 'hauteur':");
        println!("    * Type: {:?}", hauteur_col.dtype());
        println!("    * Number of values: {}", hauteur_col.len());
    }

    // Display DataFrame
    println!("  - DataFrame preview:");
    println!("{}", df);

    Ok(())
}
