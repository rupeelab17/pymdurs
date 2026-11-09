use super::{Building, BuildingCollection};
use geo::polygon;

#[test]
fn test_building_new() {
    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    let building = Building::new(poly);
    assert!(building.height.is_none());
    assert!(building.area > 0.0);
}

#[test]
fn test_building_with_height() {
    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    let building = Building::with_height(poly, 10.0);
    assert_eq!(building.height, Some(10.0));
}

#[test]
fn test_building_collection() {
    let mut collection = BuildingCollection::new_simple(None);
    assert_eq!(collection.len(), 0);

    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    let building = Building::with_height(poly, 10.0);
    collection.add_building(building);
    assert_eq!(collection.len(), 1);
}

#[test]
fn test_calculate_mean_height() {
    let mut collection = BuildingCollection::new_simple(None);

    let poly1 = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    let building1 = Building::with_height(poly1, 10.0);
    collection.add_building(building1);

    let poly2 = polygon![
        (x: 2.0, y: 0.0),
        (x: 3.0, y: 0.0),
        (x: 3.0, y: 1.0),
        (x: 2.0, y: 1.0),
        (x: 2.0, y: 0.0),
    ];
    let building2 = Building::with_height(poly2, 20.0);
    collection.add_building(building2);

    let mean_height = collection.calculate_mean_height();
    assert!((mean_height - 15.0).abs() < 0.001);
}

#[test]
fn test_process_heights() {
    let mut collection = BuildingCollection::new_simple(None);
    collection.set_default_storey_height(3.0);

    let poly1 = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    let mut building1 = Building::new(poly1);
    building1.set_nombre_d_etages(5.0);
    collection.add_building(building1);

    let poly2 = polygon![
        (x: 2.0, y: 0.0),
        (x: 3.0, y: 0.0),
        (x: 3.0, y: 1.0),
        (x: 2.0, y: 1.0),
        (x: 2.0, y: 0.0),
    ];
    let building2 = Building::with_height(poly2, 12.0);
    collection.add_building(building2);

    collection.process_heights();

    assert_eq!(collection.buildings[0].height, Some(15.0));
    assert_eq!(collection.buildings[1].height, Some(12.0));
}

#[test]
fn test_to_polars_df() {
    let mut collection = BuildingCollection::new_simple(None);

    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    let building = Building::with_height(poly, 10.0);
    collection.add_building(building);

    let df = collection.to_polars_df().unwrap();
    assert_eq!(df.height(), 1);
    assert!(df.column("hauteur").is_ok());
    assert!(df.column("area").is_ok());
}
