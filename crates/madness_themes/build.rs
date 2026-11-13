//! Compiles the madness-themes submodule into two compact JSON files in OUT_DIR:
//! `voices.json` (each voice's `shellomancy` section, flattened to dotted keys)
//! and `palettes.json` (each palette's `terminal` block). Only what Shellomancy
//! reads is embedded, not the ~130 KB of Inventorium strings per voice.

use std::path::{Path, PathBuf};
use std::{env, fs};

use serde_json::{Map, Value, json};

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../madness-themes");
    println!("cargo:rerun-if-changed={}", dir.display());
    if !dir.join("standard.json").is_file() {
        panic!(
            "madness-themes submodule is missing at {}. Run: git submodule update --init madness-themes",
            dir.display()
        );
    }

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();

    let mut voices = Vec::new();
    let mut palettes = Vec::new();
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let data: Value = serde_json::from_str(&fs::read_to_string(path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if let Some(id) = stem.strip_suffix("-colors") {
            if let Some(terminal) = data.get("terminal") {
                palettes.push(json!({
                    "id": id,
                    "name": data.get("displayName").cloned().unwrap_or(json!(id)),
                    "mode": data.get("mode").cloned().unwrap_or(json!("dark")),
                    "terminal": terminal,
                }));
            }
        } else {
            let mut strings = Map::new();
            if let Some(section) = data.get("shellomancy") {
                flatten("shellomancy", section, &mut strings, path);
            }
            voices.push(json!({
                "id": stem,
                "name": data.get("displayName").cloned().unwrap_or(json!(stem)),
                "description": data.get("description").cloned().unwrap_or(json!("")),
                "strings": strings,
            }));
        }
    }

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(
        out.join("voices.json"),
        serde_json::to_string(&voices).unwrap(),
    )
    .unwrap();
    fs::write(
        out.join("palettes.json"),
        serde_json::to_string(&palettes).unwrap(),
    )
    .unwrap();
}

fn flatten(prefix: &str, value: &Value, out: &mut Map<String, Value>, path: &Path) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                flatten(&format!("{prefix}.{k}"), v, out, path);
            }
        }
        Value::String(s) => {
            out.insert(prefix.to_owned(), Value::String(s.clone()));
        }
        other => panic!("{}: {prefix} must be a string, got {other}", path.display()),
    }
}
