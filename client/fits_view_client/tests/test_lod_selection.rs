use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Debug)]
struct LodTestCase {
    name: String,
    zoom_level: f64,
    expected_lod: i32,
    explanation: String,
}

#[derive(Deserialize, Debug)]
struct HysteresisTestCase {
    name: String,
    zoom_level: f64,
    current_lod: i32,
    threshold: f64,
    expected_lod: i32,
    explanation: String,
}

#[derive(Deserialize, Debug)]
struct TestCases {
    basic_cases: Vec<LodTestCase>,
    hysteresis_cases: Vec<HysteresisTestCase>,
}

fn calculate_lod(zoom_level: f64) -> i32 {
    if zoom_level <= 0.0 {
        return 0; // Or some other sensible default for invalid zoom
    }
    let z_ideal = -zoom_level.log2();
    z_ideal.round().max(0.0) as i32
}

fn calculate_lod_with_hysteresis(zoom_level: f64, current_lod: i32, threshold: f64) -> i32 {
    if zoom_level <= 0.0 {
        return current_lod;
    }
    let z_ideal = -zoom_level.log2();
    if (z_ideal - current_lod as f64).abs() > 0.5 + threshold {
        z_ideal.round().max(0.0) as i32
    } else {
        current_lod
    }
}

#[test]
fn test_lod_selection_from_json() {
    let path = Path::new(file!())
        .parent()
        .unwrap()
        .join("lod_test_cases.json");
    let json_str = fs::read_to_string(path).expect("Failed to read lod_test_cases.json");
    let test_cases: TestCases = serde_json::from_str(&json_str).expect("Failed to parse JSON");

    for case in test_cases.basic_cases {
        println!("Testing basic case: {}", case.name);
        let calculated_lod = calculate_lod(case.zoom_level);
        assert_eq!(calculated_lod, case.expected_lod, "Failed basic case: {}. Explanation: {}", case.name, case.explanation);
    }

    for case in test_cases.hysteresis_cases {
        println!("Testing hysteresis case: {}", case.name);
        let calculated_lod = calculate_lod_with_hysteresis(case.zoom_level, case.current_lod, case.threshold);
        assert_eq!(calculated_lod, case.expected_lod, "Failed hysteresis case: {}. Explanation: {}", case.name, case.explanation);
    }
}