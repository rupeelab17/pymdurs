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
#[cfg(feature = "polars")]
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

#[test]
fn building_get_height_from_storeys() {
    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    let mut from_storeys = Building::new(poly.clone());
    from_storeys.set_nombre_d_etages(4.0);
    assert_eq!(from_storeys.get_height(3.0), 12.0);

    let mut from_h2 = Building::new(poly.clone());
    from_h2.set_hauteur_2(7.5);
    assert_eq!(from_h2.get_height(3.0), 7.5);

    assert_eq!(Building::new(poly.clone()).get_height(3.0), 0.0);
    assert!(Building::new(poly).area_height_product().is_none());
    assert_eq!(
        Building::with_height(
            polygon![
                (x: 0.0, y: 0.0),
                (x: 2.0, y: 0.0),
                (x: 2.0, y: 2.0),
                (x: 0.0, y: 2.0),
                (x: 0.0, y: 0.0),
            ],
            5.0
        )
        .area_height_product(),
        Some(20.0)
    );
}

#[test]
fn from_geojson_parses_hauteur() {
    let geojson = br#"{
      "type": "FeatureCollection",
      "features": [
        {
          "type": "Feature",
          "properties": {"hauteur": 12.0},
          "geometry": {
            "type": "Polygon",
            "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]]
          }
        }
      ]
    }"#;
    let collection = BuildingCollection::from_geojson(geojson, None, 3.0, None).unwrap();
    assert_eq!(collection.len(), 1);
    assert_eq!(collection.buildings[0].height, Some(12.0));
}

#[test]
fn from_geojson_skips_point() {
    let geojson = br#"{
      "type": "FeatureCollection",
      "features": [
        {
          "type": "Feature",
          "properties": {},
          "geometry": {"type": "Point", "coordinates": [1, 2]}
        },
        {
          "type": "Feature",
          "properties": {"hauteur": 8},
          "geometry": {
            "type": "Polygon",
            "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]]
          }
        }
      ]
    }"#;
    let collection = BuildingCollection::from_geojson(geojson, None, 3.0, None).unwrap();
    assert_eq!(collection.len(), 1);
    assert_eq!(collection.buildings[0].height, Some(8.0));
}

#[test]
fn get_geojson_feature_props() {
    let mut collection = BuildingCollection::new_simple(None);
    let poly = polygon![
        (x: 0.0, y: 0.0),
        (x: 1.0, y: 0.0),
        (x: 1.0, y: 1.0),
        (x: 0.0, y: 1.0),
        (x: 0.0, y: 0.0),
    ];
    collection.add_building(Building::with_height(poly, 9.0));
    let gj = collection.get_geojson().unwrap();
    match gj {
        geojson::GeoJson::FeatureCollection(fc) => {
            assert_eq!(fc.features.len(), 1);
            let props = fc.features[0].properties.as_ref().unwrap();
            assert_eq!(props.get("hauteur").and_then(|v| v.as_f64()), Some(9.0));
            assert!(props.get("area").and_then(|v| v.as_f64()).unwrap() > 0.0);
        }
        _ => panic!("expected FeatureCollection"),
    }
}

#[test]
fn building_collection_set_bbox() {
    let mut collection = BuildingCollection::new_simple(None);
    collection
        .set_bbox(-1.15, 46.18, -1.14, 46.19)
        .unwrap();
    assert!(collection.geo_core.get_bbox().is_some());
}
