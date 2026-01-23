//! Admin commands for IAM management
//!
//! This module provides commands for managing users, policies, groups,
//! and service accounts on RustFS/MinIO-compatible servers.

mod group;
mod policy;
mod service_account;
mod user;

use clap::Subcommand;

use crate::exit_code::ExitCode;
use crate::output::{Formatter, OutputConfig};
use rc_core::AliasManager;
use rc_s3::AdminClient;

/// Admin subcommands for IAM management
#[derive(Subcommand, Debug)]
pub enum AdminCommands {
    /// Manage IAM users
    #[command(subcommand)]
    User(user::UserCommands),

    /// Manage IAM policies
    #[command(subcommand)]
    Policy(policy::PolicyCommands),

    /// Manage IAM groups
    #[command(subcommand)]
    Group(group::GroupCommands),

    /// Manage service accounts
    #[command(name = "service-account", subcommand)]
    ServiceAccount(service_account::ServiceAccountCommands),
}

/// Execute an admin subcommand
pub async fn execute(cmd: AdminCommands, output_config: OutputConfig) -> ExitCode {
    let formatter = Formatter::new(output_config);

    match cmd {
        AdminCommands::User(user_cmd) => user::execute(user_cmd, &formatter).await,
        AdminCommands::Policy(policy_cmd) => policy::execute(policy_cmd, &formatter).await,
        AdminCommands::Group(group_cmd) => group::execute(group_cmd, &formatter).await,
        AdminCommands::ServiceAccount(sa_cmd) => service_account::execute(sa_cmd, &formatter).await,
    }
}

/// Helper to get AdminClient from an alias name
pub fn get_admin_client(alias_name: &str, formatter: &Formatter) -> Result<AdminClient, ExitCode> {
    let alias_manager = match AliasManager::new() {
        Ok(am) => am,
        Err(e) => {
            formatter.error(&format!("Failed to load aliases: {e}"));
            return Err(ExitCode::GeneralError);
        }
    };

    let alias = match alias_manager.get(alias_name) {
        Ok(a) => a,
        Err(rc_core::Error::AliasNotFound(_)) => {
            formatter.error(&format!("Alias '{}' not found", alias_name));
            return Err(ExitCode::NotFound);
        }
        Err(e) => {
            formatter.error(&format!("Failed to get alias: {e}"));
            return Err(ExitCode::GeneralError);
        }
    };

    match AdminClient::new(&alias) {
        Ok(client) => Ok(client),
        Err(e) => {
            formatter.error(&format!("Failed to create admin client: {e}"));
            Err(ExitCode::GeneralError)
        }
    }
}
