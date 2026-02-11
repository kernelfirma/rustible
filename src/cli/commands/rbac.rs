//! RBAC command - Manage role-based access control
//!
//! Provides CLI subcommands for checking authorization, listing roles,
//! and inspecting individual role definitions.

use super::CommandContext;
use anyhow::Result;
use clap::{Parser, Subcommand};
use rustible::security::rbac::{Action, AuthzRequest, RbacConfig, RbacEngine};

/// Arguments for the rbac command
#[derive(Parser, Debug, Clone)]
pub struct RbacArgs {
    #[command(subcommand)]
    pub action: RbacAction,
}

/// RBAC subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum RbacAction {
    /// Check if a principal is authorized for an action on a resource
    Check(CheckArgs),

    /// List all configured roles
    #[command(name = "list-roles")]
    ListRoles,

    /// Show details of a specific role
    #[command(name = "show-role")]
    ShowRole(ShowRoleArgs),
}

/// Arguments for the check subcommand
#[derive(Parser, Debug, Clone)]
pub struct CheckArgs {
    /// Principal (user or service) to check
    #[arg(long)]
    pub principal: String,

    /// Resource identifier to check against
    #[arg(long)]
    pub resource: String,

    /// Action to check (read, write, execute, admin)
    #[arg(long)]
    pub action: String,

    /// Roles to assign to the principal (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub roles: Vec<String>,
}

/// Arguments for the show-role subcommand
#[derive(Parser, Debug, Clone)]
pub struct ShowRoleArgs {
    /// Name of the role to display
    pub name: String,
}

impl RbacArgs {
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        let config = RbacConfig::with_builtins();
        let mut engine = RbacEngine::new();
        engine.load_roles(config.roles.clone());

        match &self.action {
            RbacAction::Check(args) => {
                let request = AuthzRequest {
                    principal: args.principal.clone(),
                    roles: args.roles.clone(),
                    resource: args.resource.clone(),
                    action: Action::from_str(&args.action),
                };

                let decision = engine.authorize(&request);

                if decision.allowed {
                    ctx.output.success(&format!(
                        "ALLOWED: {} may {} on {}",
                        request.principal, request.action, request.resource
                    ));
                } else {
                    ctx.output.error(&format!(
                        "DENIED: {} may not {} on {}",
                        request.principal, request.action, request.resource
                    ));
                }
                ctx.output.info(&format!("Reason: {}", decision.reason));
                if let Some(role) = &decision.matched_role {
                    ctx.output.info(&format!("Matched role: {}", role));
                }

                Ok(if decision.allowed { 0 } else { 1 })
            }

            RbacAction::ListRoles => {
                ctx.output.banner("RBAC ROLES");
                for role in &config.roles {
                    ctx.output.info(&format!(
                        "  {:<12} - {} ({} permissions, inherits: [{}])",
                        role.name,
                        role.description,
                        role.permissions.len(),
                        role.inherits.join(", ")
                    ));
                }
                Ok(0)
            }

            RbacAction::ShowRole(args) => {
                if let Some(role) = config.roles.iter().find(|r| r.name == args.name) {
                    ctx.output.banner(&format!("Role: {}", role.name));
                    ctx.output.info(&format!("Description: {}", role.description));
                    if !role.inherits.is_empty() {
                        ctx.output
                            .info(&format!("Inherits: {}", role.inherits.join(", ")));
                    }
                    ctx.output.section("Permissions:");
                    for perm in &role.permissions {
                        let actions: Vec<String> =
                            perm.actions.iter().map(|a| a.to_string()).collect();
                        ctx.output.info(&format!(
                            "  {:?} {} on '{}'",
                            perm.effect,
                            actions.join(", "),
                            perm.resource.pattern
                        ));
                    }
                    Ok(0)
                } else {
                    ctx.output
                        .error(&format!("Role '{}' not found", args.name));
                    Ok(1)
                }
            }
        }
    }
}
