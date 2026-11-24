//! Guards the bundled migration list against drift from the Go tree.
//!
//! `backend/migrations` stays the source of truth for everything the Go oracle
//! applies. `011_web_sessions.sql` is Rust-only (tower-sessions storage), so it
//! is the one expected name that does not exist in the Go tree.

use std::path::PathBuf;

#[test]
fn bundled_migration_names_match_the_go_tree() {
    let backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../backend/migrations");
    let mut expected: Vec<String> = std::fs::read_dir(&backend)
        .unwrap_or_else(|error| panic!("read {}: {error}", backend.display()))
        .map(|entry| {
            entry
                .expect("read migration directory entry")
                .file_name()
                .into_string()
                .expect("migration name is UTF-8")
        })
        .filter(|name| name.ends_with(".sql"))
        .collect();
    expected.push("011_web_sessions.sql".to_string());
    expected.sort();

    assert_eq!(
        azahar_db::migration_names(),
        expected,
        "azahar-db MIGRATIONS drifted from backend/migrations"
    );
}
