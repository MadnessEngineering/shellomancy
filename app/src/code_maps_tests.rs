use serde_json::json;

use super::*;

#[test]
fn summary_reads_cartogomancy_output() {
    let map = json!({
        "classes": [{}, {}, {}],
        "packages": [{}],
        "complexityAnalysis": {
            "threatLevelDistribution": {"LOW": 1, "MEDIUM": 0, "HIGH": 2, "CRITICAL": 5}
        },
        "importAnalysis": {"unusedExports": [{}, {}]}
    });
    assert_eq!(
        summarize(&map),
        MapSummary {
            classes: 3,
            packages: 1,
            critical: 5,
            high: 2,
            unused_exports: 2,
        }
    );
}

#[test]
fn summary_tolerates_skipped_analyzers() {
    // --no-imports and friends leave whole sections out.
    let map = json!({"classes": [{}], "packages": []});
    assert_eq!(
        summarize(&map),
        MapSummary {
            classes: 1,
            ..Default::default()
        }
    );
}

#[test]
fn map_files_are_named_per_path() {
    let a = map_file_name(Path::new("/work/one/app"));
    let b = map_file_name(Path::new("/work/two/app"));
    assert!(a.starts_with("app-") && a.ends_with("-uml.json"), "{a}");
    assert_ne!(a, b);
    assert_eq!(a, map_file_name(Path::new("/work/one/app")));
    assert_eq!(
        map_file_name(Path::new("/x/my repo!")).split('-').next(),
        Some("my")
    );
    assert!(!map_file_name(Path::new("/x/my repo!")).contains(' '));
}

#[test]
fn upload_response_is_read() {
    assert_eq!(
        upload_outcome(
            200,
            &json!({"success": true, "version": 7, "viewUrl": "https://madnessinteractive.cc/dashboard"})
        ),
        UploadOutcome::Uploaded {
            version: 7,
            view_url: Some("https://madnessinteractive.cc/dashboard".to_owned()),
        }
    );
    assert_eq!(
        upload_outcome(
            413,
            &json!({"success": false, "message": "UML data too large"})
        ),
        UploadOutcome::Failed {
            error: "UML data too large (HTTP 413)".to_owned()
        }
    );
    assert_eq!(
        upload_outcome(502, &Value::Null),
        UploadOutcome::Failed {
            error: "HTTP 502".to_owned()
        }
    );
}

#[test]
fn error_tail_prefers_stderr_last_line() {
    assert_eq!(
        error_tail(
            b"warming up\n\xe2\x9d\x8c Error: Path does not exist: /nope\n\n",
            b"noise"
        ),
        "Error: Path does not exist: /nope"
    );
    assert_eq!(error_tail(b"", b"one\ntwo\n"), "two");
    assert_eq!(error_tail(b"", b""), "cartogomancy exited with an error");
    let long = "x".repeat(ERROR_TAIL_CHARS + 50);
    assert_eq!(
        error_tail(long.as_bytes(), b"").chars().count(),
        ERROR_TAIL_CHARS + 1
    );
}

#[test]
fn index_round_trips() {
    let index = Index {
        folders: vec![MappedFolder {
            path: PathBuf::from("/work/app"),
            name: "app".to_owned(),
            map_file: PathBuf::from("/maps/app-0-uml.json"),
            mapped_at: Some(1_794_600_000),
            summary: Some(MapSummary::default()),
            upload: Some(UploadOutcome::Failed {
                error: "HTTP 500".to_owned(),
            }),
            error: None,
            git: Some(GitRefs {
                git_dir: PathBuf::from("/work/app/.git"),
                default_branch: "main".to_owned(),
                signature: "deadbeef".to_owned(),
            }),
        }],
    };
    let raw = serde_json::to_string(&index).unwrap();
    assert_eq!(
        serde_json::from_str::<Index>(&raw).unwrap().folders,
        index.folders
    );
    // An entry written before a field existed still loads.
    let old: Index = serde_json::from_str(
        r#"{"folders":[{"path":"/w","name":"w","map_file":"/m/w-uml.json"}]}"#,
    )
    .unwrap();
    assert_eq!(old.folders[0].summary, None);
}

#[test]
fn only_branch_heads_tags_and_packed_refs_count() {
    assert!(is_ref_path(Path::new("/r/.git/refs/heads/main")));
    assert!(is_ref_path(Path::new("/r/.git/refs/tags/v1.2.0")));
    assert!(is_ref_path(Path::new("/r/.git/packed-refs")));
    assert!(!is_ref_path(Path::new("/r/.git/index")));
    assert!(!is_ref_path(Path::new("/r/.git/HEAD")));
    assert!(!is_ref_path(Path::new("/r/.git/objects/ab/cdef")));
    assert!(!is_ref_path(Path::new("/r/.git/refs/remotes/origin/main")));
}

#[test]
fn remaps_wait_out_the_hour() {
    let hour = AUTO_REMAP_INTERVAL_SECS;
    assert_eq!(remap_wait(None, 1_000), None);
    assert_eq!(remap_wait(Some(1_000), 1_000 + hour), None);
    assert_eq!(remap_wait(Some(1_000), 1_000 + hour + 5), None);
    assert_eq!(
        remap_wait(Some(1_000), 1_010),
        Some(Duration::from_secs((hour - 10) as u64))
    );
}

#[test]
fn fnv1a_is_stable() {
    // Map file names and ref signatures are kept on disk, so the hash must
    // not drift between builds.
    assert_eq!(fnv1a(b""), 0x811c9dc5);
    assert_eq!(fnv1a(b"a"), 0xe40c292c);
}
