//! Event Bus and Reactive Automation Engine
//!
//! This module provides an event-driven architecture for Rustible, enabling
//! reactive automation based on system events. It consists of:
//!
//! - **Event**: Core event types and structures for the event system
//! - **Bus**: Pub/sub event bus for distributing events to subscribers
//! - **Reactor**: Rule-based reactor engine for matching events to actions
//! - **Action**: Executable actions triggered by reactor rules
//! - **Reliability**: Deduplication, retry, and dead-letter queue support
//! - **Config**: Configuration for the event bus system
//!
//! # Example
//!
//! ```rust,no_run
//! use rustible::eventbus::{Event, EventType, EventSource, EventBus, ReactorEngine};
//!
//! let mut bus = EventBus::new();
//! let mut reactor = ReactorEngine::new();
//!
//! // Create and publish an event
//! let event = Event::new(
//!     EventType::PlaybookCompleted,
//!     EventSource::new("executor"),
//! );
//! bus.publish(&event);
//! ```

pub mod action;
pub mod bus;
pub mod config;
pub mod event;
pub mod reactor;
pub mod reliability;

// Re-export key types
pub use action::{ActionExecutor, ActionResult, ReactorAction};
pub use bus::{EventBus, EventFilter, EventSubscriber};
pub use config::EventBusConfig;
pub use event::{Event, EventSource, EventType};
pub use reactor::{ReactorCondition, ReactorEngine, ReactorRule};
pub use reliability::{DeadLetterEntry, DeadLetterQueue, Deduplicator, RetryPolicy};
