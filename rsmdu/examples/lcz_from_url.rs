use anyhow::Result;
use rsmdu::geometric::lcz::Lcz;

/// Example: Loading LCZ (Local Climate Zone) data from URL
/// Following Python example from pymdu.geometric.Lcz
fn main() -> Result<()> {
    println!("=== Example: Loading LCZ from URL ===\n");

    // Create Lcz instance
    // Python: lcz = Lcz()
    let mut lcz = Lcz::new(None, Some("./output".to_string()), None)?;

    // Set bounding box (La Rochelle, France)
    // Python: lcz.bbox = [-1.152704, 46.181627, -1.139893, 46.18699]
    lcz.set_bbox(-1.152704, 46.181627, -1.139893, 46.18699);

    println!("Bounding box set:");
    println!("  - Longitude: -1.152704 to -1.139893");
    println!("  - Latitude: 46.181627 to 46.18699");
    println!("  - Area: La Rochelle, France");
    println!("  - Format: WGS84 (EPSG:4326)\n");

    // Display LCZ color table
    println!("LCZ color table:");
    for (code, (name, color)) in &lcz.table_color {
        println!("  LCZ {}: {} ({})", code, name, color);
    }
    println!();

    // Run LCZ processing
    // Python: lcz = lcz.run()
    println!("Downloading and processing LCZ from URL...");
    println!("  Note: Full implementation requires GDAL shapefile reading and spatial overlay");
    lcz.run(None)?;

    println!("\n✅ Processing complete!");

    Ok(())
}
