//! Golden tests for verifying JSON output format stability
//!
//! These tests ensure that the JSON output format remains stable
//! and matches the schema defined in `schemas/output_v1.json`.
//!
//! Run with: `cargo test --features golden`

#![cfg(feature = "golden")]

use std::process::Command;

/// Get the path to the rc binary
fn rc_binary() -> String {
    // Use cargo to build and get the binary path
    let output = Command::new("cargo")
        .args(["build", "--release", "-p", "rc"])
        .output()
        .expect("Failed to build rc binary");

    if !output.status.success() {
        panic!(
            "Failed to build rc binary: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Return path to binary
    env!("CARGO_MANIFEST_DIR").to_string() + "/../../target/release/rc"
}

mod alias_tests {
    use super::*;
    use tempfile::TempDir;

    /// Set up a temporary config directory for isolated testing
    fn setup_test_env() -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        temp_dir
    }

    #[test]
    fn test_alias_list_empty_json() {
        let temp_dir = setup_test_env();
        let config_dir = temp_dir.path().to_str().unwrap();

        let output = Command::new(rc_binary())
            .args(["alias", "list", "--json"])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to execute rc");

        assert!(output.status.success(), "Command should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        // Verify structure matches schema
        insta::assert_json_snapshot!("alias_list_empty", json);
    }

    #[test]
    fn test_alias_set_json() {
        let temp_dir = setup_test_env();
        let config_dir = temp_dir.path().to_str().unwrap();

        let output = Command::new(rc_binary())
            .args([
                "alias",
                "set",
                "test-alias",
                "http://localhost:9000",
                "accesskey",
                "secretkey",
                "--json",
            ])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to execute rc");

        assert!(output.status.success(), "Command should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        // Verify structure matches schema
        insta::assert_json_snapshot!("alias_set_success", json);
    }

    #[test]
    fn test_alias_list_with_aliases_json() {
        let temp_dir = setup_test_env();
        let config_dir = temp_dir.path().to_str().unwrap();

        // First, set up some aliases
        Command::new(rc_binary())
            .args([
                "alias",
                "set",
                "local",
                "http://localhost:9000",
                "accesskey",
                "secretkey",
                "--json",
            ])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to set alias");

        Command::new(rc_binary())
            .args([
                "alias",
                "set",
                "s3",
                "https://s3.amazonaws.com",
                "awskey",
                "awssecret",
                "--region",
                "us-west-2",
                "--json",
            ])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to set alias");

        // Now list them
        let output = Command::new(rc_binary())
            .args(["alias", "list", "--json"])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to execute rc");

        assert!(output.status.success(), "Command should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        // Verify structure - aliases should be sorted by name for consistent snapshots
        assert!(json["aliases"].is_array());
        assert_eq!(json["aliases"].as_array().unwrap().len(), 2);

        insta::assert_json_snapshot!("alias_list_with_aliases", json);
    }

    #[test]
    fn test_alias_remove_json() {
        let temp_dir = setup_test_env();
        let config_dir = temp_dir.path().to_str().unwrap();

        // First, set up an alias
        Command::new(rc_binary())
            .args([
                "alias",
                "set",
                "to-remove",
                "http://localhost:9000",
                "accesskey",
                "secretkey",
                "--json",
            ])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to set alias");

        // Now remove it
        let output = Command::new(rc_binary())
            .args(["alias", "remove", "to-remove", "--json"])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to execute rc");

        assert!(output.status.success(), "Command should succeed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value =
            serde_json::from_str(&stdout).expect("Output should be valid JSON");

        insta::assert_json_snapshot!("alias_remove_success", json);
    }

    #[test]
    fn test_alias_remove_not_found_json() {
        let temp_dir = setup_test_env();
        let config_dir = temp_dir.path().to_str().unwrap();

        let output = Command::new(rc_binary())
            .args(["alias", "remove", "nonexistent", "--json"])
            .env("RC_CONFIG_DIR", config_dir)
            .output()
            .expect("Failed to execute rc");

        // Should fail with NOT_FOUND exit code (5)
        assert!(!output.status.success(), "Command should fail");
        assert_eq!(
            output.status.code(),
            Some(5),
            "Exit code should be 5 (NOT_FOUND)"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        let json: serde_json::Value =
            serde_json::from_str(&stderr).expect("Output should be valid JSON");

        insta::assert_json_snapshot!("alias_remove_not_found", json);
    }
}
