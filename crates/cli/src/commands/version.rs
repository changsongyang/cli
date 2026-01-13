//! version command - Manage bucket versioning
//!
//! Enable, disable, or check versioning status for a bucket.

use clap::{Args, Subcommand};
use rc_core::{AliasManager, ObjectStore as _};
use rc_s3::S3Client;
use serde::Serialize;

use crate::exit_code::ExitCode;
use crate::output::{Formatter, OutputConfig};

/// Manage bucket versioning
#[derive(Args, Debug)]
pub struct VersionArgs {
    #[command(subcommand)]
    pub command: VersionCommands,
}

#[derive(Subcommand, Debug)]
pub enum VersionCommands {
    /// Enable versioning for a bucket
    Enable(BucketArg),

    /// Suspend versioning for a bucket
    Suspend(BucketArg),

    /// Get versioning status for a bucket
    Info(BucketArg),

    /// List object versions
    List(ListVersionsArgs),
}

#[derive(Args, Debug)]
pub struct BucketArg {
    /// Path to the bucket (alias/bucket)
    pub path: String,

    /// Force operation even if capability detection fails
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct ListVersionsArgs {
    /// Path to list versions (alias/bucket[/prefix])
    pub path: String,

    /// Maximum number of versions to show
    #[arg(short = 'n', long, default_value = "100")]
    pub max: i32,

    /// Force operation even if capability detection fails
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Serialize)]
struct VersioningStatus {
    bucket: String,
    enabled: Option<bool>,
    status: String,
}

#[derive(Debug, Serialize)]
struct VersionInfo {
    key: String,
    version_id: String,
    is_latest: bool,
    is_delete_marker: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_human: Option<String>,
}

/// Execute the version command
pub async fn execute(args: VersionArgs, output_config: OutputConfig) -> ExitCode {
    match args.command {
        VersionCommands::Enable(bucket_arg) => execute_enable(bucket_arg, output_config).await,
        VersionCommands::Suspend(bucket_arg) => execute_suspend(bucket_arg, output_config).await,
        VersionCommands::Info(bucket_arg) => execute_info(bucket_arg, output_config).await,
        VersionCommands::List(list_args) => execute_list(list_args, output_config).await,
    }
}

async fn execute_enable(args: BucketArg, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    let (alias_name, bucket) = match parse_bucket_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    let (client, _caps) = match setup_client(&alias_name, &bucket, args.force, &formatter).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    match client.set_versioning(&bucket, true).await {
        Ok(()) => {
            if formatter.is_json() {
                let output = VersioningStatus {
                    bucket: bucket.clone(),
                    enabled: Some(true),
                    status: "Enabled".to_string(),
                };
                formatter.json(&output);
            } else {
                formatter.println(&format!("Versioning enabled for bucket '{bucket}'"));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to enable versioning: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_suspend(args: BucketArg, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    let (alias_name, bucket) = match parse_bucket_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    let (client, _caps) = match setup_client(&alias_name, &bucket, args.force, &formatter).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    match client.set_versioning(&bucket, false).await {
        Ok(()) => {
            if formatter.is_json() {
                let output = VersioningStatus {
                    bucket: bucket.clone(),
                    enabled: Some(false),
                    status: "Suspended".to_string(),
                };
                formatter.json(&output);
            } else {
                formatter.println(&format!("Versioning suspended for bucket '{bucket}'"));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to suspend versioning: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_info(args: BucketArg, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    let (alias_name, bucket) = match parse_bucket_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    let (client, _caps) = match setup_client(&alias_name, &bucket, args.force, &formatter).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    match client.get_versioning(&bucket).await {
        Ok(status) => {
            let (enabled, status_str) = match status {
                Some(true) => (Some(true), "Enabled"),
                Some(false) => (Some(false), "Suspended"),
                None => (None, "Not configured"),
            };

            if formatter.is_json() {
                let output = VersioningStatus {
                    bucket: bucket.clone(),
                    enabled,
                    status: status_str.to_string(),
                };
                formatter.json(&output);
            } else {
                formatter.println(&format!("Bucket: {bucket}"));
                formatter.println(&format!("Versioning: {status_str}"));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to get versioning status: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_list(args: ListVersionsArgs, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    let (alias_name, bucket, prefix) = match parse_version_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    let (client, _caps) = match setup_client(&alias_name, &bucket, args.force, &formatter).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let path = rc_core::RemotePath::new(&alias_name, &bucket, prefix.as_deref().unwrap_or(""));

    match client.list_object_versions(&path, Some(args.max)).await {
        Ok(versions) => {
            if formatter.is_json() {
                let output: Vec<VersionInfo> = versions
                    .into_iter()
                    .map(|v| VersionInfo {
                        key: v.key,
                        version_id: v.version_id,
                        is_latest: v.is_latest,
                        is_delete_marker: v.is_delete_marker,
                        last_modified: v.last_modified.map(|t| t.to_string()),
                        size_bytes: v.size_bytes,
                        size_human: v
                            .size_bytes
                            .map(|s| humansize::format_size(s as u64, humansize::BINARY)),
                    })
                    .collect();
                formatter.json(&output);
            } else if versions.is_empty() {
                formatter.println("No versions found.");
            } else {
                for v in &versions {
                    let marker = if v.is_delete_marker { "[DELETE]" } else { "" };
                    let latest = if v.is_latest { "*" } else { " " };
                    let size = v
                        .size_bytes
                        .map(|s| humansize::format_size(s as u64, humansize::BINARY))
                        .unwrap_or_default();

                    formatter.println(&format!(
                        "{latest} {:<40} {:>10} {:>12} {marker}",
                        v.key,
                        v.version_id.chars().take(10).collect::<String>(),
                        size
                    ));
                }
                formatter.println(&format!("\nTotal: {} version(s)", versions.len()));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to list versions: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn setup_client(
    alias_name: &str,
    bucket: &str,
    force: bool,
    formatter: &Formatter,
) -> Result<(S3Client, rc_core::Capabilities), ExitCode> {
    let alias_manager = match AliasManager::new() {
        Ok(am) => am,
        Err(e) => {
            formatter.error(&format!("Failed to load aliases: {e}"));
            return Err(ExitCode::GeneralError);
        }
    };

    let alias = match alias_manager.get(alias_name) {
        Ok(a) => a,
        Err(_) => {
            formatter.error(&format!("Alias '{alias_name}' not found"));
            return Err(ExitCode::NotFound);
        }
    };

    let client = match S3Client::new(alias).await {
        Ok(c) => c,
        Err(e) => {
            formatter.error(&format!("Failed to create S3 client: {e}"));
            return Err(ExitCode::NetworkError);
        }
    };

    // Check capabilities
    let caps = match client.capabilities().await {
        Ok(c) => c,
        Err(e) => {
            if force {
                rc_core::Capabilities::default()
            } else {
                formatter.error(&format!("Failed to detect capabilities: {e}"));
                return Err(ExitCode::NetworkError);
            }
        }
    };

    if !force && !caps.versioning {
        formatter.error("Backend does not support versioning. Use --force to attempt anyway.");
        return Err(ExitCode::UnsupportedFeature);
    }

    // Check if bucket exists
    match client.bucket_exists(bucket).await {
        Ok(true) => {}
        Ok(false) => {
            formatter.error(&format!("Bucket '{bucket}' does not exist"));
            return Err(ExitCode::NotFound);
        }
        Err(e) => {
            formatter.error(&format!("Failed to check bucket: {e}"));
            return Err(ExitCode::NetworkError);
        }
    }

    Ok((client, caps))
}

fn parse_bucket_path(path: &str) -> Result<(String, String), String> {
    if path.is_empty() {
        return Err("Path cannot be empty".to_string());
    }

    let parts: Vec<&str> = path.splitn(2, '/').collect();

    if parts.len() < 2 || parts[1].is_empty() {
        return Err("Bucket name is required (alias/bucket)".to_string());
    }

    // Remove trailing slash from bucket name
    let bucket = parts[1].trim_end_matches('/');

    Ok((parts[0].to_string(), bucket.to_string()))
}

fn parse_version_path(path: &str) -> Result<(String, String, Option<String>), String> {
    if path.is_empty() {
        return Err("Path cannot be empty".to_string());
    }

    let parts: Vec<&str> = path.splitn(3, '/').collect();

    match parts.len() {
        1 => Err("Bucket name is required".to_string()),
        2 => Ok((parts[0].to_string(), parts[1].to_string(), None)),
        3 => Ok((
            parts[0].to_string(),
            parts[1].to_string(),
            Some(parts[2].to_string()),
        )),
        _ => Err(format!("Invalid path format: '{path}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bucket_path() {
        let (alias, bucket) = parse_bucket_path("myalias/mybucket").unwrap();
        assert_eq!(alias, "myalias");
        assert_eq!(bucket, "mybucket");

        let (alias, bucket) = parse_bucket_path("myalias/mybucket/").unwrap();
        assert_eq!(alias, "myalias");
        assert_eq!(bucket, "mybucket");
    }

    #[test]
    fn test_parse_bucket_path_errors() {
        assert!(parse_bucket_path("").is_err());
        assert!(parse_bucket_path("myalias").is_err());
        assert!(parse_bucket_path("myalias/").is_err());
    }

    #[test]
    fn test_parse_version_path() {
        let (alias, bucket, prefix) = parse_version_path("myalias/mybucket").unwrap();
        assert_eq!(alias, "myalias");
        assert_eq!(bucket, "mybucket");
        assert!(prefix.is_none());

        let (alias, bucket, prefix) = parse_version_path("myalias/mybucket/path/to").unwrap();
        assert_eq!(alias, "myalias");
        assert_eq!(bucket, "mybucket");
        assert_eq!(prefix, Some("path/to".to_string()));
    }
}
