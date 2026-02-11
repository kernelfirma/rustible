//! Failure-injection and chaos testing infrastructure.
//!
//! This module provides tools for injecting controlled failures into Rustible
//! operations, enabling systematic resilience testing. It is gated behind the
//! `hpc` feature flag since it is test infrastructure rather than production
//! functionality.
//!
//! # Components
//!
//! - [`fault`]: Fault definitions and injector traits for simulating failures.
//! - [`connection`]: A chaos layer that can wrap connection-like operations to
//!   inject delays and errors.
//! - [`scorecard`]: Reliability scorecards and regression gates for tracking
//!   scenario outcomes.

pub mod connection;
pub mod fault;
pub mod scorecard;

pub use connection::ChaosLayer;
pub use fault::{CompositeFaultInjector, Fault, FaultInjector, SimpleFaultInjector};
pub use scorecard::{
    RegressionGate, ReliabilityScorecard, ScenarioCategory, ScenarioResult,
};
