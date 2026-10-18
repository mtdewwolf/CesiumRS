//! Time-dependent Transforms frames, Earth orientation parameters, IAU 2006
//! XYS data, and IAU orientation against CesiumJS fixtures.

mod common;

use cesium_rs::core::earth_orientation_parameters::EopData;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

fn jd(v: &Value) -> JulianDate {
    JulianDate::new(n(&v[0]), n(&v[1]), TimeStandard::Tai)
}

/// Every chunk of Cesium's XYS data, from `assets/IAU2006_XYS`.
fn xys() -> Iau2006XysData {
    let mut xys = Iau2006XysData::default();
    for chunk in 0..28 {
        let path = format!("{}/assets/IAU2006_XYS/IAU2006_XYS_{chunk}.json", env!("CARGO_MANIFEST_DIR"));
        xys.add_chunk_json(chunk, &std::fs::read_to_string(path).unwrap()).unwrap();
    }
    xys
}

/// The synthetic EOP table stored in the `data` fixture case. Negative zero
/// is encoded as "-0", so cells go through the fixture number decoder.
fn eop_data() -> EopData {
    let cases = load("core", "EarthOrientationParameters");
    let data = &cases.iter().find(|c| c.f == "data").unwrap().r;
    EopData {
        column_names: list(&data["columnNames"], |c| s(c).to_string()),
        samples: list(&data["samples"], |v| if v.is_number() || v == "-0" { n(v) } else { f64::NAN }),
    }
}

#[test]
fn transforms_time() {
    let xys = xys();
    let none = EarthOrientationParameters::none();
    let eop = EarthOrientationParameters::new(&eop_data(), false).unwrap();
    run(
        "core",
        "TransformsTime",
        |_| TIGHT,
        |f, a| {
            let d = jd(&a[0]);
            match f {
                "computeTemeToPseudoFixedMatrix" => ok(Transforms::compute_teme_to_pseudo_fixed_matrix(&d)),
                "computeMoonFixedToIcrfMatrix" => ok(Transforms::compute_moon_fixed_to_icrf_matrix(&d)),
                "computeIcrfToMoonFixedMatrix" => ok(Transforms::compute_icrf_to_moon_fixed_matrix(&d)),
                "computeFixedToIcrfMatrix" => ok(Transforms::compute_fixed_to_icrf_matrix(&d, &none, &xys)),
                "computeIcrfToFixedMatrix" => ok(Transforms::compute_icrf_to_fixed_matrix(&d, &none, &xys)),
                "computeIcrfToCentralBodyFixedMatrix" => {
                    ok(Transforms::compute_icrf_to_central_body_fixed_matrix(&d, &none, &xys))
                }
                "computeFixedToIcrfMatrixEop" => ok(Transforms::compute_fixed_to_icrf_matrix(&d, &eop, &xys)),
                _ => None,
            }
        },
    );
}

#[test]
fn iau2006_xys_data() {
    let xys = xys();
    run(
        "core",
        "Iau2006XysData",
        |_| TIGHT,
        |f, a| match f {
            "computeXysRadians" => ok(xys.compute_xys_radians(n(&a[0]), n(&a[1])).map(|r| [r.x, r.y, r.s])),
            _ => None,
        },
    );
}

#[test]
fn iau_orientation() {
    let axes = IauOrientationAxes::default();
    run(
        "core",
        "IauOrientation",
        |_| TIGHT,
        |f, a| match f {
            "computeMoon" => {
                let p = cesium_rs::core::iau2000_orientation::compute_moon(&jd(&a[0]));
                ok([p.right_ascension, p.declination, p.rotation, p.rotation_rate])
            }
            "evaluate" => ok(axes.evaluate(&jd(&a[0]))),
            _ => None,
        },
    );
}

fn sample_enc(s: EarthOrientationParametersSample) -> [f64; 5] {
    [s.x_pole_wander, s.y_pole_wander, s.x_pole_offset, s.y_pole_offset, s.ut1_minus_utc]
}

#[test]
fn earth_orientation_parameters() {
    let data = eop_data();
    let table = load("core", "EarthOrientationParameters").into_iter().find(|c| c.f == "data").unwrap().r;
    run(
        "core",
        "EarthOrientationParameters",
        |_| TIGHT,
        |f, a| match f {
            // The table is input for the other cases, not a result to check.
            "data" => ok(table.clone()),
            "compute" => {
                let eop = EarthOrientationParameters::new(&data, false).unwrap();
                ok(list(&a[0], |d| sample_enc(eop.compute(&jd(d)))).to_vec())
            }
            "computeNone" | "computeEmpty" => ok(sample_enc(EarthOrientationParameters::none().compute(&jd(&a[0])))),
            "leapSecondsAfterLoad" => {
                let before = JulianDate::leap_seconds().len();
                EarthOrientationParameters::new(&data, true).unwrap();
                let table = JulianDate::leap_seconds();
                let added = table.len() as f64 - before as f64;
                let rows: Vec<Value> = table
                    .iter()
                    .map(|l| json!([num(l.julian_date.day_number), num(l.julian_date.seconds_of_day), num(l.offset)]))
                    .collect();
                ok(json!([added, rows]))
            }
            _ => None,
        },
    );
}

#[test]
fn xys_requests_missing_chunks() {
    let mut xys = Iau2006XysData::default();
    // 2015-01-01 TT is in chunk 14.
    assert!(xys.compute_xys_radians(2457023.0, 43200.0).is_none());
    assert_eq!(xys.take_requested_chunks(), vec![14]);
    assert!(xys.take_requested_chunks().is_empty());
    let path = format!("{}/assets/IAU2006_XYS/IAU2006_XYS_14.json", env!("CARGO_MANIFEST_DIR"));
    xys.add_chunk_json(14, &std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(xys.compute_xys_radians(2457023.0, 43200.0).is_some());
    assert_eq!(xys.chunk_url(3), "Assets/IAU2006_XYS/IAU2006_XYS_3.json");
}

#[cfg(feature = "bundled-xys")]
#[test]
fn bundled_xys_matches_asset_files() {
    let bundled = Iau2006XysData::bundled();
    let files = xys();
    for day in [2442400.0, 2451545.0, 2460000.0, 2469000.0] {
        assert_eq!(bundled.compute_xys_radians(day, 1234.5), files.compute_xys_radians(day, 1234.5));
    }
}
