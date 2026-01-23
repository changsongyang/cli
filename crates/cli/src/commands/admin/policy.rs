//! Policy management commands
//!
//! Commands for managing IAM policies: list, create, info, remove, attach.

use clap::Subcommand;
use serde::Serialize;
use std::fs;

use super::get_admin_client;
use crate::exit_code::ExitCode;
use crate::output::Formatter;
use rc_core::admin::{AdminApi, PolicyEntity};

/// Policy management subcommands
#[derive(Subcommand, Debug)]
pub enum PolicyCommands {
    /// List all policies
    #[command(name = "ls", alias = "list")]
    List(ListArgs),

    /// Create a new policy
    Create(CreateArgs),

    /// Get policy information
    Info(InfoArgs),

    /// Remove a policy
    #[command(name = "rm", alias = "remove")]
    Remove(RemoveArgs),

    /// Attach policy to a user or group
    Attach(AttachArgs),
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Alias name of the server
    pub alias: String,
}

#[derive(clap::Args, Debug)]
pub struct CreateArgs {
    /// Alias name of the server
    pub alias: String,

    /// Policy name
    pub name: String,

    /// Path to policy JSON file
    pub policy_file: String,
}

#[derive(clap::Args, Debug)]
pub struct InfoArgs {
    /// Alias name of the server
    pub alias: String,

    /// Policy name
    pub name: String,
}

#[derive(clap::Args, Debug)]
pub struct RemoveArgs {
    /// Alias name of the server
    pub alias: String,

    /// Policy name to remove
    pub name: String,
}

#[derive(clap::Args, Debug)]
pub struct AttachArgs {
    /// Alias name of the server
    pub alias: String,

    /// Policy name(s) to attach (comma-separated for multiple)
    pub policies: String,

    /// Target user access key
    #[arg(long, conflicts_with = "group")]
    pub user: Option<String>,

    /// Target group name
    #[arg(long, conflicts_with = "user")]
    pub group: Option<String>,
}

/// JSON output for policy list
#[derive(Serialize)]
struct PolicyListOutput {
    policies: Vec<String>,
}

/// JSON output for policy info
#[derive(Serialize)]
struct PolicyInfoOutput {
    name: String,
    policy: serde_json::Value,
}

/// JSON output for policy operations
#[derive(Serialize)]
struct PolicyOperationOutput {
    success: bool,
    name: String,
    message: String,
}

/// Execute a policy subcommand
pub async fn execute(cmd: PolicyCommands, formatter: &Formatter) -> ExitCode {
    match cmd {
        PolicyCommands::List(args) => execute_list(args, formatter).await,
        PolicyCommands::Create(args) => execute_create(args, formatter).await,
        PolicyCommands::Info(args) => execute_info(args, formatter).await,
        PolicyCommands::Remove(args) => execute_remove(args, formatter).await,
        PolicyCommands::Attach(args) => execute_attach(args, formatter).await,
    }
}

async fn execute_list(args: ListArgs, formatter: &Formatter) -> ExitCode {
    let client = match get_admin_client(&args.alias, formatter) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match client.list_policies().await {
        Ok(policies) => {
            if formatter.is_json() {
                let output = PolicyListOutput {
                    policies: policies.into_iter().map(|p| p.name).collect(),
                };
                formatter.json(&output);
            } else if policies.is_empty() {
                formatter.println("No policies found.");
            } else {
                for policy in policies {
                    let styled_name = formatter.style_name(&policy.name);
                    formatter.println(&format!("  {styled_name}"));
                }
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to list policies: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_create(args: CreateArgs, formatter: &Formatter) -> ExitCode {
    let client = match get_admin_client(&args.alias, formatter) {
        Ok(c) => c,
        Err(code) => return code,
    };

    if args.name.is_empty() {
        formatter.error("Policy name cannot be empty");
        return ExitCode::UsageError;
    }

    // Read policy file
    let policy_content = match fs::read_to_string(&args.policy_file) {
        Ok(content) => content,
        Err(e) => {
            formatter.error(&format!(
                "Failed to read policy file '{}': {e}",
                args.policy_file
            ));
            return ExitCode::UsageError;
        }
    };

    // Validate JSON
    if serde_json::from_str::<serde_json::Value>(&policy_content).is_err() {
        formatter.error("Policy file is not valid JSON");
        return ExitCode::UsageError;
    }

    match client.create_policy(&args.name, &policy_content).await {
        Ok(()) => {
            if formatter.is_json() {
                let output = PolicyOperationOutput {
                    success: true,
                    name: args.name.clone(),
                    message: format!("Policy '{}' created successfully", args.name),
                };
                formatter.json(&output);
            } else {
                let styled_name = formatter.style_name(&args.name);
                formatter.success(&format!("Policy '{styled_name}' created successfully."));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to create policy: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_info(args: InfoArgs, formatter: &Formatter) -> ExitCode {
    let client = match get_admin_client(&args.alias, formatter) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match client.get_policy(&args.name).await {
        Ok(policy) => {
            if formatter.is_json() {
                let policy_value: serde_json::Value = policy
                    .parse_document()
                    .unwrap_or(serde_json::Value::String(policy.policy.clone()));
                let output = PolicyInfoOutput {
                    name: policy.name,
                    policy: policy_value,
                };
                formatter.json(&output);
            } else {
                let styled_name = formatter.style_name(&policy.name);
                formatter.println(&format!("Policy: {styled_name}"));
                formatter.println("");
                formatter.println(&policy.policy);
            }
            ExitCode::Success
        }
        Err(rc_core::Error::NotFound(_)) => {
            formatter.error(&format!("Policy '{}' not found", args.name));
            ExitCode::NotFound
        }
        Err(e) => {
            formatter.error(&format!("Failed to get policy info: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_remove(args: RemoveArgs, formatter: &Formatter) -> ExitCode {
    let client = match get_admin_client(&args.alias, formatter) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match client.delete_policy(&args.name).await {
        Ok(()) => {
            if formatter.is_json() {
                let output = PolicyOperationOutput {
                    success: true,
                    name: args.name.clone(),
                    message: format!("Policy '{}' removed successfully", args.name),
                };
                formatter.json(&output);
            } else {
                let styled_name = formatter.style_name(&args.name);
                formatter.success(&format!("Policy '{styled_name}' removed successfully."));
            }
            ExitCode::Success
        }
        Err(rc_core::Error::NotFound(_)) => {
            formatter.error(&format!("Policy '{}' not found", args.name));
            ExitCode::NotFound
        }
        Err(e) => {
            formatter.error(&format!("Failed to remove policy: {e}"));
            ExitCode::GeneralError
        }
    }
}

async fn execute_attach(args: AttachArgs, formatter: &Formatter) -> ExitCode {
    let client = match get_admin_client(&args.alias, formatter) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let (entity_type, entity_name) = match (&args.user, &args.group) {
        (Some(user), None) => (PolicyEntity::User, user.clone()),
        (None, Some(group)) => (PolicyEntity::Group, group.clone()),
        _ => {
            formatter.error("Must specify either --user or --group");
            return ExitCode::UsageError;
        }
    };

    let policy_names: Vec<String> = args
        .policies
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if policy_names.is_empty() {
        formatter.error("At least one policy name is required");
        return ExitCode::UsageError;
    }

    match client
        .attach_policy(&policy_names, entity_type, &entity_name)
        .await
    {
        Ok(()) => {
            let entity_desc = match entity_type {
                PolicyEntity::User => format!("user '{}'", entity_name),
                PolicyEntity::Group => format!("group '{}'", entity_name),
            };
            if formatter.is_json() {
                let output = PolicyOperationOutput {
                    success: true,
                    name: policy_names.join(","),
                    message: format!(
                        "Policy '{}' attached to {} successfully",
                        policy_names.join(","),
                        entity_desc
                    ),
                };
                formatter.json(&output);
            } else {
                let styled_policies = formatter.style_name(&policy_names.join(", "));
                formatter.success(&format!(
                    "Policy '{styled_policies}' attached to {entity_desc} successfully."
                ));
            }
            ExitCode::Success
        }
        Err(e) => {
            formatter.error(&format!("Failed to attach policy: {e}"));
            ExitCode::GeneralError
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_list_output_serialization() {
        let output = PolicyListOutput {
            policies: vec!["readonly".to_string(), "admin".to_string()],
        };
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("readonly"));
        assert!(json.contains("admin"));
    }
}
