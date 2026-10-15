use std::env;
use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=brand.env");
    println!("cargo:rerun-if-env-changed=SHIPYARD_VERSION");
    let text = fs::read_to_string("brand.env").expect("brand.env is missing");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            println!("cargo:rustc-env=BRAND_{}={}", key.trim(), value.trim().trim_matches('"'));
        }
    }
    let version = env::var("SHIPYARD_VERSION")
        .ok()
        .map(|v| v.trim().trim_start_matches(['v', 'V']).to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=BRAND_VERSION={version}");
}
