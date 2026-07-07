use anyhow::Result;
use rsmdu::geometric::{mnh::Mnh, mns::Mns, mnt::Mnt};

/// Example: Download LiDAR HD elevation models (MNT, MNS, MNH) from IGN WMS-R API.
fn main() -> Result<()> {
    println!("=== LiDAR HD elevation models from IGN WMS-R ===\n");

    // Atlantec / La Rochelle area (EPSG:4326)
    let bbox = (-1.153414, 46.180217, -1.141098, 46.186531);
    let output_path = "./output/lidar_hd_elevation";

    println!("Bounding box (WGS84): {:?}", bbox);
    println!("Note: data is only available where LiDAR HD has been acquired.\n");

    println!("Downloading MNT (terrain)...");
    let mut mnt = Mnt::new(Some(output_path.to_string()))?;
    mnt.set_bbox(bbox.0, bbox.1, bbox.2, bbox.3);
    mnt.set_crs(2154);
    let mnt = mnt.run()?;
    println!("  MNT: {:?}", mnt.get_path_save_tiff());

    println!("Downloading MNS (surface)...");
    let mut mns = Mns::new(Some(output_path.to_string()))?;
    mns.set_bbox(bbox.0, bbox.1, bbox.2, bbox.3);
    mns.set_crs(2154);
    let mns = mns.run()?;
    println!("  MNS: {:?}", mns.get_path_save_tiff());

    println!("Downloading MNH (height)...");
    let mut mnh = Mnh::new(Some(output_path.to_string()))?;
    mnh.set_bbox(bbox.0, bbox.1, bbox.2, bbox.3);
    mnh.set_crs(2154);
    let mnh = mnh.run()?;
    println!("  MNH: {:?}", mnh.get_path_save_tiff());

    println!("\nAll LiDAR HD elevation rasters downloaded.");
    Ok(())
}
