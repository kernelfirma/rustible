//! Event bus CLI commands
//!
//! Provides subcommands for interacting with the event bus system:
//! listening for events, publishing events, managing reactor rules,
//! and inspecting the dead-letter queue.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::collections::HashMap;

use rustible::eventbus::{
    DeadLetterQueue, Event, EventBus, EventSource, EventType, ReactorAction, ReactorCondition,
    ReactorEngine, ReactorRule,
};

use super::CommandContext;

/// Arguments for the event command
#[derive(Parser, Debug, Clone)]
pub struct EventArgs {
    /// Event subcommand
    #[command(subcommand)]
    pub command: EventCommand,
}

/// Event subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum EventCommand {
    /// Listen for events on the bus
    Listen(ListenArgs),
    /// Publish an event to the bus
    Publish(PublishArgs),
    /// List all reactor rules
    #[command(name = "rules-list")]
    RulesList,
    /// Check which rules would fire for a given event
    #[command(name = "rules-check")]
    RulesCheck(RulesCheckArgs),
    /// List entries in the dead-letter queue
    #[command(name = "dlq-list")]
    DeadLetterList,
    /// Retry a dead-letter queue entry
    #[command(name = "dlq-retry")]
    DeadLetterRetry(DeadLetterRetryArgs),
}

/// Arguments for the listen subcommand
#[derive(Parser, Debug, Clone)]
pub struct ListenArgs {
    /// Filter by event type (e.g., "playbook.started", "host.down")
    #[arg(short = 't', long)]
    pub event_type: Option<String>,
}

/// Arguments for the publish subcommand
#[derive(Parser, Debug, Clone)]
pub struct PublishArgs {
    /// Event type to publish (e.g., "playbook.started", "host.down")
    #[arg(short = 't', long)]
    pub event_type: String,

    /// JSON payload for the event (e.g., '{"host": "web01"}')
    #[arg(short = 'p', long, default_value = "{}")]
    pub payload: String,
}

/// Arguments for the rules-check subcommand
#[derive(Parser, Debug, Clone)]
pub struct RulesCheckArgs {
    /// Event as JSON to evaluate against rules
    pub event_json: String,
}

/// Arguments for the dlq-retry subcommand
#[derive(Parser, Debug, Clone)]
pub struct DeadLetterRetryArgs {
    /// ID of the dead-letter entry to retry
    pub entry_id: String,
}

impl EventArgs {
    /// Execute the event subcommand
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            EventCommand::Listen(args) => execute_listen(args, ctx).await,
            EventCommand::Publish(args) => execute_publish(args, ctx).await,
            EventCommand::RulesList => execute_rules_list(ctx).await,
            EventCommand::RulesCheck(args) => execute_rules_check(args, ctx).await,
            EventCommand::DeadLetterList => execute_dlq_list(ctx).await,
            EventCommand::DeadLetterRetry(args) => execute_dlq_retry(args, ctx).await,
        }
    }
}

async fn execute_listen(args: &ListenArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("EVENT BUS - LISTEN");

    if let Some(ref event_type_str) = args.event_type {
        let event_type = EventType::from_str_loose(event_type_str);
        ctx.output
            .info(&format!("Filtering for event type: {}", event_type));
    } else {
        ctx.output.info("Listening for all event types");
    }

    ctx.output.info(
        "Event bus listener is not yet connected to a live event stream. \
         Use 'rustible event publish' to emit test events.",
    );

    Ok(0)
}

async fn execute_publish(args: &PublishArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("EVENT BUS - PUBLISH");

    let event_type = EventType::from_str_loose(&args.event_type);
    let payload: HashMap<String, serde_json::Value> = serde_json::from_str(&args.payload)
        .map_err(|e| anyhow::anyhow!("Invalid JSON payload: {}", e))?;

    let event = Event::with_payload(event_type.clone(), EventSource::new("cli"), payload);

    ctx.output
        .info(&format!("Publishing event: {}", event_type));
    ctx.output.info(&format!("  ID: {}", event.id));
    ctx.output
        .info(&format!("  Timestamp: {}", event.timestamp));

    if !event.payload.is_empty() {
        ctx.output.info(&format!(
            "  Payload: {}",
            serde_json::to_string_pretty(&event.payload)?
        ));
    }

    // Create bus and publish (no persistent subscribers in this demo)
    let bus = EventBus::new();
    bus.publish(&event);

    ctx.output.success("Event published successfully");
    Ok(0)
}

async fn execute_rules_list(ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("REACTOR RULES");

    // In a full implementation, rules would be loaded from configuration.
    // For now, show the built-in example rules.
    let engine = build_example_reactor();
    let rules = engine.list_rules();

    if rules.is_empty() {
        ctx.output.info("No reactor rules configured.");
        return Ok(0);
    }

    for (i, rule) in rules.iter().enumerate() {
        let status = if rule.enabled { "enabled" } else { "disabled" };
        ctx.output
            .info(&format!("  {}. {} [{}]", i + 1, rule.name, status,));
        ctx.output
            .info(&format!("     Condition: {:?}", rule.condition));
        ctx.output.info(&format!("     Action: {:?}", rule.action));
    }

    ctx.output
        .info(&format!("\nTotal: {} rule(s)", rules.len()));
    Ok(0)
}

async fn execute_rules_check(args: &RulesCheckArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("REACTOR RULES CHECK");

    let event: Event = serde_json::from_str(&args.event_json)
        .map_err(|e| anyhow::anyhow!("Invalid event JSON: {}", e))?;

    ctx.output.info(&format!(
        "Checking event '{}' (type: {}) against rules...",
        event.id, event.event_type
    ));

    let engine = build_example_reactor();
    let actions = engine.evaluate(&event);

    if actions.is_empty() {
        ctx.output.info("No rules matched this event.");
    } else {
        ctx.output
            .info(&format!("{} rule(s) would fire:", actions.len()));
        for (i, action) in actions.iter().enumerate() {
            ctx.output.info(&format!("  {}. {:?}", i + 1, action));
        }
    }

    Ok(0)
}

async fn execute_dlq_list(ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("DEAD-LETTER QUEUE");

    // In a full implementation, the DLQ would be persisted. For now, show empty.
    let dlq = DeadLetterQueue::new();

    if dlq.is_empty() {
        ctx.output
            .info("Dead-letter queue is empty. No failed events.");
        return Ok(0);
    }

    for entry in dlq.list() {
        ctx.output.info(&format!(
            "  {} | {} | retries: {} | {}",
            entry.id, entry.event.event_type, entry.retry_count, entry.error_message,
        ));
    }

    Ok(0)
}

async fn execute_dlq_retry(args: &DeadLetterRetryArgs, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("DEAD-LETTER QUEUE - RETRY");

    let mut dlq = DeadLetterQueue::new();

    match dlq.retry(&args.entry_id) {
        Some(entry) => {
            ctx.output.info(&format!(
                "Retrying dead-letter entry: {} (event type: {})",
                entry.id, entry.event.event_type
            ));
            ctx.output.success("Entry removed from DLQ for retry");
            Ok(0)
        }
        None => {
            ctx.output.error(&format!(
                "No dead-letter entry found with ID: {}",
                args.entry_id
            ));
            Ok(1)
        }
    }
}

/// Build an example reactor engine with sample rules for demonstration.
fn build_example_reactor() -> ReactorEngine {
    let mut engine = ReactorEngine::new();

    engine.add_rule(ReactorRule::new(
        "auto-remediate-on-drift",
        ReactorCondition::EventTypeMatch(EventType::DriftDetected),
        ReactorAction::RunPlaybook {
            path: "remediate.yml".to_string(),
        },
    ));

    engine.add_rule(ReactorRule::new(
        "notify-on-failure",
        ReactorCondition::EventTypeMatch(EventType::PlaybookFailed),
        ReactorAction::Notify {
            channel: "ops".to_string(),
            message: "Playbook execution failed".to_string(),
        },
    ));

    engine.add_rule(ReactorRule::new(
        "failover-on-host-down",
        ReactorCondition::EventTypeMatch(EventType::HostDown),
        ReactorAction::RunPlaybook {
            path: "failover.yml".to_string(),
        },
    ));

    engine
}
