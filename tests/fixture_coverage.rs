//! Prevent newly generated fixture files from silently missing Rust replay.
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn every_fixture_file_has_a_replay_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut registered = BTreeSet::new();
    for file in std::fs::read_dir(&root).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(path).unwrap();
        // Replay suites call common::run with literal package/module names.
        for call in source.split("run(").skip(1) {
            let mut args = call.split(',');
            let literal = |s: &str| s.trim().strip_prefix('"').and_then(|s| s.strip_suffix('"')).map(str::to_owned);
            if let (Some(package), Some(module)) = (args.next().and_then(literal), args.next().and_then(literal)) {
                registered.insert((package, module));
            }
        }
    }
    let mut missing = Vec::new();
    for package in std::fs::read_dir(root.join("fixtures")).unwrap() {
        let package = package.unwrap();
        if !package.file_type().unwrap().is_dir() {
            continue;
        }
        for file in std::fs::read_dir(package.path()).unwrap() {
            let path = file.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let key = (
                    package.file_name().to_string_lossy().into_owned(),
                    path.file_stem().unwrap().to_string_lossy().into_owned(),
                );
                if !registered.contains(&key) {
                    missing.push(key);
                }
            }
        }
    }
    assert!(missing.is_empty(), "fixture files without a common::run replay: {missing:?}");
}
