//! tree command - Display objects in tree format
//!
//! Shows a tree view of objects in a bucket or prefix.

use clap::Args;
use rc_core::{AliasManager, ListOptions, ObjectInfo, ObjectStore as _, RemotePath};
use rc_s3::S3Client;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::exit_code::ExitCode;
use crate::output::{Formatter, OutputConfig};

/// Display objects in tree format
#[derive(Args, Debug)]
pub struct TreeArgs {
    /// Path to display (alias/bucket[/prefix])
    pub path: String,

    /// Maximum depth to display
    #[arg(short = 'L', long, default_value = "3")]
    pub level: usize,

    /// Show file sizes
    #[arg(short, long)]
    pub size: bool,

    /// Show only directories
    #[arg(short, long)]
    pub dirs_only: bool,

    /// Pattern to include (glob-style)
    #[arg(short = 'P', long)]
    pub pattern: Option<String>,

    /// Show full path prefix
    #[arg(short, long)]
    pub full_path: bool,
}

#[derive(Debug, Serialize)]
struct TreeOutput {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    children: Option<Vec<TreeOutput>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_human: Option<String>,
    is_dir: bool,
}

struct TreeStats {
    dirs: usize,
    files: usize,
    total_size: i64,
}

/// Execute the tree command
pub async fn execute(args: TreeArgs, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    // Parse path
    let (alias_name, bucket, prefix) = match parse_tree_path(&args.path) {
        Ok(p) => p,
        Err(e) => {
            formatter.error(&e);
            return ExitCode::UsageError;
        }
    };

    // Load alias
    let alias_manager = match AliasManager::new() {
        Ok(am) => am,
        Err(e) => {
            formatter.error(&format!("Failed to load aliases: {e}"));
            return ExitCode::GeneralError;
        }
    };

    let alias = match alias_manager.get(&alias_name) {
        Ok(a) => a,
        Err(_) => {
            formatter.error(&format!("Alias '{alias_name}' not found"));
            return ExitCode::NotFound;
        }
    };

    // Create S3 client
    let client = match S3Client::new(alias).await {
        Ok(c) => c,
        Err(e) => {
            formatter.error(&format!("Failed to create S3 client: {e}"));
            return ExitCode::NetworkError;
        }
    };

    // Compile pattern if provided
    let pattern = if let Some(ref p) = args.pattern {
        match glob::Pattern::new(p) {
            Ok(pat) => Some(pat),
            Err(e) => {
                formatter.error(&format!("Invalid pattern: {e}"));
                return ExitCode::UsageError;
            }
        }
    } else {
        None
    };

    // List objects
    let remote_path = RemotePath::new(&alias_name, &bucket, prefix.as_deref().unwrap_or(""));
    let objects = match list_all_objects(&client, &remote_path).await {
        Ok(o) => o,
        Err(e) => {
            formatter.error(&format!("Failed to list objects: {e}"));
            return ExitCode::NetworkError;
        }
    };

    // Build tree structure
    let root_name = if args.full_path {
        args.path.clone()
    } else {
        prefix.clone().unwrap_or_else(|| bucket.clone())
    };

    let base_prefix = prefix.as_deref().unwrap_or("");
    let (tree, stats) = build_tree(&objects, base_prefix, &root_name, &args, pattern.as_ref());

    if formatter.is_json() {
        formatter.json(&tree);
    } else {
        // Print tree
        print_tree(&tree, "", true, &formatter, args.size);

        // Print summary
        formatter.println("");
        formatter.println(&format!(
            "{} directories, {} files",
            stats.dirs, stats.files
        ));
        if args.size {
            formatter.println(&format!(
                "Total size: {}",
                humansize::format_size(stats.total_size as u64, humansize::BINARY)
            ));
        }
    }

    ExitCode::Success
}

async fn list_all_objects(
    client: &S3Client,
    path: &RemotePath,
) -> Result<Vec<ObjectInfo>, rc_core::Error> {
    let mut all_objects = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let options = ListOptions {
            recursive: true,
            max_keys: Some(1000),
            continuation_token: continuation_token.clone(),
            ..Default::default()
        };

        let result = client.list_objects(path, options).await?;
        all_objects.extend(result.items);

        if result.truncated {
            continuation_token = result.continuation_token;
        } else {
            break;
        }
    }

    Ok(all_objects)
}

fn build_tree(
    objects: &[ObjectInfo],
    base_prefix: &str,
    root_name: &str,
    args: &TreeArgs,
    pattern: Option<&glob::Pattern>,
) -> (TreeOutput, TreeStats) {
    let mut tree: BTreeMap<String, TreeNode> = BTreeMap::new();
    let mut stats = TreeStats {
        dirs: 0,
        files: 0,
        total_size: 0,
    };

    for obj in objects {
        // Remove base prefix from key
        let relative_key = obj.key.strip_prefix(base_prefix).unwrap_or(&obj.key);
        let relative_key = relative_key.trim_start_matches('/');

        if relative_key.is_empty() {
            continue;
        }

        // Check pattern
        if let Some(pat) = pattern {
            let filename = relative_key.rsplit('/').next().unwrap_or(relative_key);
            if !pat.matches(filename) {
                continue;
            }
        }

        // Skip files if dirs_only
        if args.dirs_only && !obj.is_dir {
            continue;
        }

        // Check depth
        let depth = relative_key.matches('/').count() + 1;
        if depth > args.level {
            continue;
        }

        // Build path components
        let parts: Vec<&str> = relative_key.split('/').collect();
        insert_into_tree(&mut tree, &parts, obj, &mut stats);
    }

    let children = if tree.is_empty() {
        None
    } else {
        Some(tree_to_output(&tree, args.size))
    };

    (
        TreeOutput {
            name: root_name.to_string(),
            children,
            size_bytes: None,
            size_human: None,
            is_dir: true,
        },
        stats,
    )
}

#[derive(Debug)]
struct TreeNode {
    name: String,
    children: BTreeMap<String, TreeNode>,
    size_bytes: Option<i64>,
    size_human: Option<String>,
    is_dir: bool,
}

fn insert_into_tree(
    tree: &mut BTreeMap<String, TreeNode>,
    parts: &[&str],
    obj: &ObjectInfo,
    stats: &mut TreeStats,
) {
    if parts.is_empty() {
        return;
    }

    let name = parts[0].to_string();

    if parts.len() == 1 {
        // Leaf node
        let is_dir = obj.is_dir || name.ends_with('/');
        if is_dir {
            stats.dirs += 1;
        } else {
            stats.files += 1;
            if let Some(size) = obj.size_bytes {
                stats.total_size += size;
            }
        }

        tree.insert(
            name.clone(),
            TreeNode {
                name,
                children: BTreeMap::new(),
                size_bytes: obj.size_bytes,
                size_human: obj.size_human.clone(),
                is_dir,
            },
        );
    } else {
        // Intermediate directory
        let entry = tree.entry(name.clone()).or_insert_with(|| {
            stats.dirs += 1;
            TreeNode {
                name,
                children: BTreeMap::new(),
                size_bytes: None,
                size_human: None,
                is_dir: true,
            }
        });
        insert_into_tree(&mut entry.children, &parts[1..], obj, stats);
    }
}

fn tree_to_output(tree: &BTreeMap<String, TreeNode>, show_size: bool) -> Vec<TreeOutput> {
    tree.values()
        .map(|node| {
            let children = if node.children.is_empty() {
                None
            } else {
                Some(tree_to_output(&node.children, show_size))
            };

            TreeOutput {
                name: node.name.clone(),
                children,
                size_bytes: if show_size { node.size_bytes } else { None },
                size_human: if show_size {
                    node.size_human.clone()
                } else {
                    None
                },
                is_dir: node.is_dir,
            }
        })
        .collect()
}

fn print_tree(
    node: &TreeOutput,
    prefix: &str,
    is_last: bool,
    formatter: &Formatter,
    show_size: bool,
) {
    // Print current node
    let connector = if prefix.is_empty() {
        ""
    } else if is_last {
        "└── "
    } else {
        "├── "
    };

    let size_str = if show_size && !node.is_dir {
        node.size_human
            .as_ref()
            .map(|s| format!(" [{s}]"))
            .unwrap_or_default()
    } else {
        String::new()
    };

    let name = if node.is_dir && !node.name.ends_with('/') {
        format!("{}/", node.name)
    } else {
        node.name.clone()
    };

    formatter.println(&format!("{prefix}{connector}{name}{size_str}"));

    // Print children
    if let Some(ref children) = node.children {
        let new_prefix = if prefix.is_empty() {
            String::new()
        } else if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}│   ")
        };

        for (i, child) in children.iter().enumerate() {
            let child_is_last = i == children.len() - 1;
            print_tree(child, &new_prefix, child_is_last, formatter, show_size);
        }
    }
}

/// Parse tree path into (alias, bucket, prefix)
fn parse_tree_path(path: &str) -> Result<(String, String, Option<String>), String> {
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
    fn test_parse_tree_path() {
        let (alias, bucket, prefix) = parse_tree_path("myalias/mybucket").unwrap();
        assert_eq!(alias, "myalias");
        assert_eq!(bucket, "mybucket");
        assert!(prefix.is_none());
    }

    #[test]
    fn test_parse_tree_path_with_prefix() {
        let (alias, bucket, prefix) = parse_tree_path("myalias/mybucket/path/to").unwrap();
        assert_eq!(alias, "myalias");
        assert_eq!(bucket, "mybucket");
        assert_eq!(prefix, Some("path/to".to_string()));
    }

    #[test]
    fn test_parse_tree_path_errors() {
        assert!(parse_tree_path("").is_err());
        assert!(parse_tree_path("myalias").is_err());
    }
}
