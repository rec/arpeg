use arpeg_midi::{Profile, parse_profile};
use std::path::Path;

#[test]
fn profile_headers_match_shared_defaults_and_errors() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/profile-headers.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let text = case["text"].as_str().unwrap();
        let path = case
            .get("path")
            .and_then(toml::Value::as_str)
            .map(Path::new);
        let result = parse_profile(text, path);
        if let Some(error) = case.get("error").and_then(toml::Value::as_str) {
            assert!(
                result.err().expect("invalid preset").contains(error),
                "{}",
                case["name"]
            );
        } else {
            let Profile::Classic(profile) = result.expect("valid preset") else {
                panic!("expected classic preset");
            };
            assert_eq!(
                profile.chance.name,
                case["expected_name"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }
}
