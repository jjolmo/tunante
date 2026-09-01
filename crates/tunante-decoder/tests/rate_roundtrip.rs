//! The rate subcommand, end to end: run the real binary on a real file and
//! read the rating back through the same resolver the apps use.

use std::path::Path;
use std::process::Command;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tunante-codec/tests/fixtures")
        .join(name)
}

fn temp_copy(name: &str, tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tunante-rate-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dst = dir.join(name);
    std::fs::copy(fixture(name), &dst).unwrap();
    dst
}

fn run_rate(path: &Path, rating: i32, order: &str) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_tunante-decoder"))
        .args(["rate", &path.to_string_lossy(), &rating.to_string(), "--order", order])
        .output()
        .unwrap();
    assert!(out.status.success(), "rate failed: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// A FLAC takes the rating in its own tag.
#[test]
fn a_flac_stores_the_rating_in_the_file() {
    let path = temp_copy("sine.flac", "flac");
    let v = run_rate(&path, 5, "file,folder");
    assert_eq!(v["stored_in"], "file");

    use tunante_codec::metadata::rating_source::{parse_order, resolve_rating};
    let order = parse_order(Some("file"));
    assert_eq!(resolve_rating(&path.to_string_lossy(), 0, &order), 5);

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A format whose tag cannot take it falls through to the folder's
/// `_ratings.m3u` — the fallback is the feature, not a failure.
#[test]
fn an_untaggable_file_falls_through_to_the_folder() {
    let path = temp_copy("sample.nsf", "nsf");
    let v = run_rate(&path, 3, "file,folder");
    assert_eq!(v["stored_in"], "folder");

    use tunante_codec::metadata::rating_source::{parse_order, resolve_rating};
    let order = parse_order(Some("folder"));
    assert_eq!(resolve_rating(&path.to_string_lossy(), 0, &order), 3);

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// An order that starts with db means "don't touch the disk".
#[test]
fn a_db_first_order_never_touches_the_disk() {
    let path = temp_copy("sine.flac", "dbfirst");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let v = run_rate(&path, 2, "db,file,folder");
    assert_eq!(v["stored_in"], "db");
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);
    assert!(!path.parent().unwrap().join("_ratings.m3u").exists());

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
