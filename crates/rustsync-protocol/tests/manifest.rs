use rustsync_protocol::{
    BlobId, Manifest, ManifestEntry, ProtocolError, UnixTimestamp, WorkspaceId,
};

#[test]
fn manifest_entries_round_trip_unix_timestamp_numbers() {
    let workspace_id = WorkspaceId::parse("workspace_test123").expect("valid workspace id");
    let mut manifest = Manifest::new(workspace_id.clone());

    manifest
        .insert(
            "docs/readme.md".to_string(),
            ManifestEntry::file(
                42,
                "sha256:abc123".to_string(),
                UnixTimestamp::from_secs(1_700_000_000),
            ),
        )
        .expect("valid file path");
    manifest
        .insert("docs".to_string(), ManifestEntry::directory())
        .expect("valid directory path");

    let json = serde_json::to_string(&manifest).expect("serialize manifest");
    let decoded: Manifest = serde_json::from_str(&json).expect("deserialize manifest");

    assert_eq!(decoded.workspace_id, workspace_id);
    assert_eq!(decoded.entries, manifest.entries);
    assert!(json.contains("\"modified_at\":1700000000"));
}

#[test]
fn manifest_file_entry_accepts_previous_modified_at_field_name() {
    let json = r#"
    {
        "workspace_id": "workspace_test123",
        "entries": {
            "docs/readme.md": {
                "File": {
                    "size": 42,
                    "content_hash": "sha256:abc123",
                    "modified_at": 1700000000
                }
            }
        }
    }"#;

    let decoded: Manifest = serde_json::from_str(json).expect("deserialize manifest");
    let entry = decoded.get("docs/readme.md").expect("manifest entry");
    let ManifestEntry::File(file) = entry else {
        panic!("expected file entry");
    };

    assert_eq!(file.modified_at.as_secs(), 1_700_000_000);
}

#[test]
fn manifest_file_entry_exposes_modified_at_as_timestamp_type() {
    let entry = ManifestEntry::file(5, "sha256:def456".to_string(), UnixTimestamp::from_secs(99));

    let ManifestEntry::File(file) = entry else {
        panic!("expected file entry");
    };

    assert_eq!(file.modified_at.as_secs(), 99);
}

#[test]
fn manifest_file_entry_skips_empty_remote_blob_id() {
    let entry = ManifestEntry::file(5, "sha256:def456".to_string(), UnixTimestamp::from_secs(99));

    let json = serde_json::to_string(&entry).expect("serialize entry");

    assert!(!json.contains("remote_blob_id"), "{json}");
}

#[test]
fn manifest_file_entry_serializes_remote_blob_id_when_present() {
    let mut entry =
        ManifestEntry::file(5, "sha256:def456".to_string(), UnixTimestamp::from_secs(99));
    let remote_blob_id = BlobId::from_content(b"encrypted blob");
    let ManifestEntry::File(file) = &mut entry else {
        panic!("expected file entry");
    };
    file.remote_blob_id = Some(remote_blob_id.clone());

    let json = serde_json::to_string(&entry).expect("serialize entry");
    let decoded: ManifestEntry = serde_json::from_str(&json).expect("deserialize entry");

    assert!(json.contains("remote_blob_id"), "{json}");
    let ManifestEntry::File(file) = decoded else {
        panic!("expected file entry");
    };
    assert_eq!(file.remote_blob_id, Some(remote_blob_id));
}

#[test]
fn manifest_rejects_paths_that_escape_or_are_not_normalized() {
    let workspace_id = WorkspaceId::parse("workspace_test123").expect("valid workspace id");
    let mut manifest = Manifest::new(workspace_id);

    for path in [
        "",
        "/absolute",
        "../outside",
        "docs/../outside",
        "docs//readme.md",
        "docs\\readme.md",
    ] {
        assert!(matches!(
            manifest
                .insert(path.to_string(), ManifestEntry::directory())
                .expect_err("invalid manifest path must be rejected"),
            ProtocolError::InvalidManifestPath { .. }
        ));
    }
}
