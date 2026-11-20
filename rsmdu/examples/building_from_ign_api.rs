// Example: Using BuildingCollection with IGN API
// This example shows how to load buildings from the French IGN API
//
// IMPORTANT:
// - IGN API requires an internet connection
// - The API may have rate limiting
// - Bounding box must be in WGS84 (EPSG:4326)
// - For production use you may need to register and get an API key at https://geoservices.ign.fr/
//
use anyhow::Result;
use rsmdu::geo_core::BoundingBox;
use rsmdu::geometric::building::BuildingCollection;

fn main() -> Result<()> {
    println!("=== Example: Loading buildings from IGN API ===\n");

    // Set bounding box (geographic area)
    // Example: Area around La Rochelle, France
    // Format: [min_x, min_y, max_x, max_y] in decimal degrees (WGS84, EPSG:4326)
    let Bbox = BoundingBox::new(
        -1.152704, // Min longitude (West)
        46.181627, // Min latitude (South)
        -1.139893, // Max longitude (East)
        46.18699,  // Max latitude (North)
    );

    println!("Bounding box set:");
    println!("  - Longitude: {} to {}", Bbox.min_x, Bbox.max_x);
    println!("  - Latitude: {} to {}", Bbox.min_y, Bbox.max_y);
    println!("  - Area: La Rochelle, France");
    println!("  - Format: WGS84 (EPSG:4326)\n");

    // Load buildings from IGN API
    println!("Loading buildings from IGN API...");
    println!("  - Service: WFS (Web Feature Service)");
    println!("  - Layer: BDTOPO_V3:batiment");
    println!("  - Format: GeoJSON");
    println!("(This may take a few seconds)\n");

    match BuildingCollection::from_ign_api(
        Some("./output".to_string()), // Output folder
        3.0,                          // Default storey height (3 meters)
        Some(Bbox),                   // Bounding box
    ) {
        Ok(mut collection) => {
            println!("✓ Buildings loaded successfully!\n");

            // Display statistics
            println!("Statistics:");
            println!("  - Number of buildings: {}", collection.len());

            if collection.is_empty() {
                println!("\n⚠ No buildings found in this area.");
                println!("  Try a different bounding box or check your internet connection.");
                return Ok(());
            }

            // Display details for the first 5 buildings
            println!("\nDetails of first 5 buildings:");
            for (idx, building) in collection.buildings().iter().take(5).enumerate() {
                println!("  Building {}:", idx + 1);
                println!("    - Height: {:?} m", building.height);
                println!("    - Area: {:.2} m²", building.area);
                println!(
                    "    - Centroid: ({:.6}, {:.6})",
                    building.centroid.x(),
                    building.centroid.y()
                );
            }

            // Compute mean height before processing
            let mean_height_before = collection.calculate_mean_height();
            println!(
                "\nMean height (before processing): {:.2} m",
                mean_height_before
            );

            // Process heights (fill missing heights)
            println!("\nProcessing heights...");
            collection.process_heights();

            // Display statistics after processing
            let mean_height_after = collection.calculate_mean_height();
            println!("Mean height (after processing): {:.2} m", mean_height_after);

            // Height statistics
            let buildings_with_height: usize = collection
                .buildings()
                .iter()
                .filter(|b| b.height.is_some())
                .count();
            println!(
                "Buildings with height: {} / {}",
                buildings_with_height,
                collection.len()
            );

            // Convert to Polars DataFrame
            println!("\nConverting to Polars DataFrame...");
            match collection.to_polars_df() {
                Ok(df) => {
                    println!("✓ DataFrame created successfully!");
                    println!("  - Number of rows: {}", df.height());
                    println!("  - Number of columns: {}", df.width());
                    println!("  - Columns: {:?}", df.get_column_names());

                    // Display preview
                    println!("\nDataFrame preview (first 5 rows):");
                    println!("{}", df.head(Some(5)));
                }
                Err(e) => {
                    eprintln!("⚠ Error converting to DataFrame: {}", e);
                }
            }

            // Example: Filter buildings by height
            println!("\nExample: Buildings over 15 meters:");
            let tall_buildings: Vec<_> = collection
                .buildings()
                .iter()
                .filter(|b| b.height.map_or(false, |h| h > 15.0))
                .collect();
            println!("  Number of buildings > 15m: {}", tall_buildings.len());

            // Example: Compute total area
            let total_area: f64 = collection.buildings().iter().map(|b| b.area).sum();
            println!(
                "\nTotal building area: {:.2} m² ({:.2} hectares)",
                total_area,
                total_area / 10000.0
            );
        }
        Err(e) => {
            eprintln!("✗ Error loading from IGN API:");
            eprintln!("  {}", e);
            eprintln!("\nPossible checks:");
            eprintln!("  - Check your internet connection");
            eprintln!("  - Check that the bounding box is valid");
            eprintln!("  - IGN API may have rate limits");
            return Err(e);
        }
    }

    Ok(())
}

/// Example with another geographic area
#[allow(dead_code)]
fn example_paris() -> Result<()> {
    // Area around Paris (example)
    let Bbox = BoundingBox::new(
        2.30,  // Min longitude
        48.85, // Min latitude
        2.35,  // Max longitude
        48.87, // Max latitude
    );

    let collection =
        BuildingCollection::from_ign_api(Some("./output".to_string()), 3.0, Some(Bbox))?;

    println!("Buildings loaded for Paris: {}", collection.len());
    Ok(())
}

/// Example with coordinate transformation
#[allow(dead_code)]
fn example_with_crs_transform() -> Result<()> {
    // Bounding box in WGS84 (EPSG:4326)
    let Bbox_wgs84 = BoundingBox::new(-1.152704, 46.181627, -1.139893, 46.18699);

    // Transform to Lambert-93 (EPSG:2154) if needed
    // Note: IGN API typically returns data in WGS84
    let Bbox_lambert = Bbox_wgs84.transform(4326, 2154)?;

    println!("Bounding box WGS84: {:?}", Bbox_wgs84);
    println!("Bounding box Lambert-93: {:?}", Bbox_lambert);

    // Load buildings
    let mut collection =
        BuildingCollection::from_ign_api(Some("./output".to_string()), 3.0, Some(Bbox_wgs84))?;

    // Set collection CRS
    collection.set_crs(2154); // Lambert-93

    Ok(())
}
