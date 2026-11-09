//! Profile `Lidar::run` stage timings (cold then hot in-memory points).
//!
//! Prefer a warm LAZ/COPC disk cache under `./output/.cache/laz` so timings
//! reflect decompress/parse rather than network.
//!
//! ```text
//! cargo run -p rsmdu --example lidar_run_profile --release
//! ```

use anyhow::Result;
use rsmdu::geometric::lidar::Lidar;

fn main() -> Result<()> {
    let _ = rsmdu::logging::init("rsmdu::lidar_timing=info,info");

    // Small La Rochelle / Atlantec emprise (WGS84)
    let bbox = (-1.153414, 46.180217, -1.141098, 46.186531);
    let output = "./output/lidar_run_profile";

    println!("=== Lidar::run stage profile ===\n");
    println!("bbox={:?}\n", bbox);

    let mut lidar = Lidar::new(Some(output.to_string()), None, None)?;
    lidar.set_bbox(bbox.0, bbox.1, bbox.2, bbox.3)?;

    println!("--- pass 1: cold points (may hit disk/network cache) ---");
    let _ = lidar.run(Some("profile_pass1.tif".into()), None, Some(1.0), false)?;

    println!("\n--- pass 2: hot points (in-memory loaded_points) ---");
    let _ = lidar.run(Some("profile_pass2.tif".into()), None, Some(1.0), false)?;

    println!("\nDone. Grep stderr for `lidar_timing`.");
    Ok(())
}
