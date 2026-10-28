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
        // Dedicated replay suites may embed their fixture directly.
        for call in source.split("include_str!(\"").skip(1) {
            let Some(relative) = call.split('"').next().and_then(|p| p.strip_prefix("fixtures/")) else {
                continue;
            };
            let path = Path::new(relative);
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let (Some(package), Some(module)) = (path.parent(), path.file_stem()) {
                    registered.insert((package.to_string_lossy().into_owned(), module.to_string_lossy().into_owned()));
                }
            }
        }
        // Shared replay suites call common::run with literal package/module names.
        for call in source.split("run(").skip(1) {
            let mut args = call.split(',');
            let literal = |s: &str| s.trim().strip_prefix('"').and_then(|s| s.strip_suffix('"')).map(str::to_owned);
            if let (Some(package), Some(module)) = (args.next().and_then(literal), args.next().and_then(literal)) {
                registered.insert((package, module));
            }
        }
    }
    let fixtures = root.join("fixtures");
    let mut found = Vec::new();
    collect_fixture_keys(&fixtures, &fixtures, &mut found);
    let missing: Vec<_> = found.into_iter().filter(|key| !registered.contains(key)).collect();
    assert!(missing.is_empty(), "fixture files without a replay suite: {missing:?}");
}

/// `(package, module)` for every fixture JSON file. A nested shader fixture
/// `shaders/builtin/pi.json` is `("shaders/builtin", "pi")`, matching
/// `run("shaders/builtin", "pi", ...)`.
fn collect_fixture_keys(dir: &Path, root: &Path, keys: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_fixture_keys(&path, root, keys);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let package = path
            .strip_prefix(root)
            .unwrap()
            .parent()
            .unwrap()
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let module = path.file_stem().unwrap().to_string_lossy().into_owned();
        keys.push((package, module));
    }
}
