use std::fs;

#[test]
fn fetch_script_only_declares_supported_api_levels() {
    let script = fs::read_to_string("tools/fetch_redroid_bundles.sh").unwrap();
    let versions = script
        .split("versions=(")
        .nth(1)
        .and_then(|value| value.split(")").next())
        .unwrap();
    let entries: Vec<_> = versions
        .lines()
        .filter_map(|line| line.trim().strip_prefix('"'))
        .filter_map(|line| line.strip_suffix('"'))
        .collect();
    assert_eq!(entries, ["35:15:15.0.0-latest", "36:16:16.0.0-latest"]);
    assert!(script.contains("tier=\"primary\""));
    assert!(script.contains("tier=\"secondary\""));
}
