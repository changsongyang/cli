//! tag command - Manage object tags
//!
//! Get, set, or remove tags on S3 objects.

use clap::{Args, Subcommand};
use rc_core::{AliasManager, ObjectStore as _, RemotePath};
use rc_s3::S3Client;
use serde::Serialize;
use std::collections::HashMap;

use crate::exit_code::ExitCode;
use crate::output::{Formatter, OutputConfig};

/// Manage object tags
#[derive(Args, Debug)]
pub struct TagArgs {
    #[command(subcommand)]
    pub command: TagCommands,
}

#[derive(Subcommand, Debug)]
pub enum TagCommands {
    /// List tags for an object
    List(ObjectPathArg),

    /// Set tags for an object
    Set(SetTagArgs),

    /// Remove all tags from an object
    Remove(ObjectPathArg),
}

#[derive(Args, Debug)]
pub struct ObjectPathArg {
    /// Path to the object (alias/bucket/key)
    pub path: String,

    /// Force operation even if capability detection fails
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct SetTagArgs {
    /// Path to the object (alias/bucket/key)
    pub path: String,

    /// Tags to set (key=value format, can specify multiple)
    #[arg(short, long, value_name = "KEY=VALUE", num_args = 1..)]
    pub tags: Vec<String>,

    /// Force operation even if capability detection fails
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Serialize)]
struct TagOutput {
    path: String,
    tags: HashMap<String, String>,
    count: usize,
}

/// Execute the tag command
pub async fn execute(args: TagArgs, output_config: OutputConfig) -> ExitCode {
    match args.command {
        TagCommands::List(path_arg) => execute_list(path_arg, output_config).await,
        TagCommands::Set(set_args) => execute_set(set_args, output_config).await,
        TagCommands::Remove(path_arg) => execute_remove(path_arg, output_config).await,
    }
}

async fn execute_list(args: ObjectPathArg, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    let (alias_name, bucket, key) = match parse_object_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    let client = match setup_client(&alias_name, args.force, &formatter).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let path = RemotePath::new(&alias_name, &bucket, &key);

    match client.get_object_tags(&path).await {
        Ok(tags) => {
            if formatter.is_json() {
                let output = TagOutput {
                    path: args.path.clone(),
                    count: tags.len(),
                    tags,
                };
                formatter.json(&output);
            } else if tags.is_empty() {
                formatter.println("No tags found.");
            } else {
                formatter.println(&format!("Tags for '{}':", args.path));
                for (k, v) in &tags {
                    formatter.println(&format!("  {k}={v}"));
                }
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to get tags: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_set(args: SetTagArgs, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    if args.tags.is_empty() {
        formatter.error("At least one tag is required (--tags key=value)");
        return ExitCode::UsageError;
    }

    let (alias_name, bucket, key) = match parse_object_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    // Parse tags
    let mut tags = HashMap::new();
    for tag_str in &args.tags {
        match tag_str.split_once('=') {
            Some((k, v)) => {
                if k.is_empty() {
                    formatter.error(&format!(
                        "Invalid tag format: '{tag_str}' (key cannot be empty)"
                    ));
                    return ExitCode::UsageError;
                }
                tags.insert(k.to_string(), v.to_string());
            }
            None => {
                formatter.error(&format!(
                    "Invalid tag format: '{tag_str}' (expected key=value)"
                ));
                return ExitCode::UsageError;
            }
        }
    }

    let client = match setup_client(&alias_name, args.force, &formatter).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let path = RemotePath::new(&alias_name, &bucket, &key);

    match client.set_object_tags(&path, tags.clone()).await {
        Ok(()) => {
            if formatter.is_json() {
                let output = TagOutput {
                    path: args.path.clone(),
                    count: tags.len(),
                    tags,
                };
                formatter.json(&output);
            } else {
                formatter.println(&format!("Set {} tag(s) on '{}'", tags.len(), args.path));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to set tags: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_remove(args: ObjectPathArg, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    let (alias_name, bucket, key) = match parse_object_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    let client = match setup_client(&alias_name, args.force, &formatter).await {
        Ok(c) => c,
        Err(code) => return code,
    };

    let path = RemotePath::new(&alias_name, &bucket, &key);

    match client.delete_object_tags(&path).await {
        Ok(()) => {
            if formatter.is_json() {
                let output = serde_json::json!({
                    "path": args.path,
                    "status": "removed"
                });
                formatter.json(&output);
            } else {
                formatter.println(&format!("Removed all tags from '{}'", args.path));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to remove tags: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn setup_client(
    alias_name: &str,
    force: bool,
    formatter: &Formatter,
) -> Result<S3Client, ExitCode> {
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
    if !force {
        match client.capabilities().await {
            Ok(caps) => {
                if !caps.tagging {
                    formatter
                        .error("Backend does not support tagging. Use --force to attempt anyway.");
                    return Err(ExitCode::UnsupportedFeature);
                }
            }
            Err(e) => {
                formatter.error(&format!("Failed to detect capabilities: {e}"));
                return Err(ExitCode::NetworkError);
            }
        }
    }

    Ok(client)
}

fn parse_object_path(path: &str) -> Result<(String, String, String), String> {
    if path.is_empty() {
        return Err("Path cannot be empty".to_string());
    }

    let parts: Vec<&str> = path.splitn(3, '/').collect();

    if parts.len() < 3 || parts[2].is_empty() {
        return Err("Object key is required (alias/bucket/key)".to_string());
    }

    Ok((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_object_path() {
        let (alias, bucket, key) = parse_object_path("myalias/mybucket/path/to/file.txt").unwrap();
        assert_eq!(alias, "myalias");
        assert_eq!(bucket, "mybucket");
        assert_eq!(key, "path/to/file.txt");
    }

    #[test]
    fn test_parse_object_path_errors() {
        assert!(parse_object_path("").is_err());
        assert!(parse_object_path("myalias").is_err());
        assert!(parse_object_path("myalias/mybucket").is_err());
        assert!(parse_object_path("myalias/mybucket/").is_err());
    }
}
