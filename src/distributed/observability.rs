//! Observability endpoints for distributed execution
//!
//! This module provides HTTP endpoints for monitoring the distributed
//! controller cluster, including health checks, metrics, and status.

use super::types::{ControllerId, ControllerLoad, ControllerRole};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Overall health status
    pub status: HealthStatus,
    /// Controller ID
    pub controller_id: String,
    /// Controller role
    pub role: String,
    /// Uptime in seconds
    pub uptime_seconds: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Component health checks
    pub checks: HashMap<String, ComponentHealth>,
}

/// Health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// All components healthy
    Healthy,
    /// Some components degraded but functional
    Degraded,
    /// Critical failure
    Unhealthy,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unhealthy => write!(f, "unhealthy"),
        }
    }
}

/// Component health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    /// Component status
    pub status: HealthStatus,
    /// Optional message
    pub message: Option<String>,
    /// Last check time
    pub last_check_ms: u64,
}

impl ComponentHealth {
    /// Create a healthy component
    pub fn healthy() -> Self {
        Self {
            status: HealthStatus::Healthy,
            message: None,
            last_check_ms: now_ms(),
        }
    }

    /// Create a degraded component
    pub fn degraded(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Degraded,
            message: Some(message.into()),
            last_check_ms: now_ms(),
        }
    }

    /// Create an unhealthy component
    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self {
            status: HealthStatus::Unhealthy,
            message: Some(message.into()),
            last_check_ms: now_ms(),
        }
    }
}

/// Cluster status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatusResponse {
    /// Cluster ID
    pub cluster_id: String,
    /// Current leader ID
    pub leader_id: Option<String>,
    /// This controller's role
    pub local_role: String,
    /// Total number of controllers
    pub total_controllers: usize,
    /// Number of healthy controllers
    pub healthy_controllers: usize,
    /// Cluster has quorum
    pub has_quorum: bool,
    /// Current Raft term
    pub current_term: u64,
    /// Controller details
    pub controllers: Vec<ControllerStatusInfo>,
}

/// Individual controller status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerStatusInfo {
    /// Controller ID
    pub id: String,
    /// Address
    pub address: String,
    /// Role
    pub role: String,
    /// Health status
    pub health: String,
    /// Region
    pub region: Option<String>,
    /// Load metrics
    pub load: LoadMetrics,
    /// Last heartbeat age in milliseconds
    pub last_heartbeat_age_ms: Option<u64>,
}

/// Load metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoadMetrics {
    /// Active work units
    pub active_work_units: u32,
    /// Active connections
    pub active_connections: u32,
    /// CPU usage percentage
    pub cpu_usage: f32,
    /// Memory usage percentage
    pub memory_usage: f32,
    /// Queue depth
    pub queue_depth: u32,
    /// Load score (0.0 - 1.0)
    pub load_score: f64,
}

impl From<&ControllerLoad> for LoadMetrics {
    fn from(load: &ControllerLoad) -> Self {
        Self {
            active_work_units: load.active_work_units,
            active_connections: load.active_connections,
            cpu_usage: load.cpu_usage,
            memory_usage: load.memory_usage,
            queue_depth: load.queue_depth,
            load_score: load.load_score(),
        }
    }
}

/// Work unit status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkUnitStatusResponse {
    /// Total work units
    pub total: usize,
    /// Pending work units
    pub pending: usize,
    /// Running work units
    pub running: usize,
    /// Completed work units
    pub completed: usize,
    /// Failed work units
    pub failed: usize,
    /// Work unit details (limited)
    pub work_units: Vec<WorkUnitInfo>,
}

/// Individual work unit info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkUnitInfo {
    /// Work unit ID
    pub id: String,
    /// Run ID
    pub run_id: String,
    /// State
    pub state: String,
    /// Assigned controller
    pub assigned_to: Option<String>,
    /// Number of hosts
    pub host_count: usize,
    /// Number of tasks
    pub task_count: usize,
    /// Progress percentage
    pub progress: f64,
    /// Priority
    pub priority: u32,
}

/// Metrics response (Prometheus format compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsResponse {
    /// Metrics in key-value format
    pub metrics: HashMap<String, MetricValue>,
}

/// Metric value with optional labels
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetricValue {
    /// Simple gauge/counter value
    Simple(f64),
    /// Value with labels
    Labeled {
        value: f64,
        labels: HashMap<String, String>,
    },
}

/// Prometheus format metric
#[derive(Debug, Clone)]
pub struct PrometheusMetric {
    /// Metric name
    pub name: String,
    /// Metric type (gauge, counter, histogram)
    pub metric_type: &'static str,
    /// Help text
    pub help: String,
    /// Values with labels
    pub values: Vec<(f64, HashMap<String, String>)>,
}

impl PrometheusMetric {
    /// Create a new gauge metric
    pub fn gauge(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            metric_type: "gauge",
            help: help.into(),
            values: Vec::new(),
        }
    }

    /// Create a new counter metric
    pub fn counter(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            metric_type: "counter",
            help: help.into(),
            values: Vec::new(),
        }
    }

    /// Add a value without labels
    pub fn with_value(mut self, value: f64) -> Self {
        self.values.push((value, HashMap::new()));
        self
    }

    /// Add a value with labels
    pub fn with_labeled_value(mut self, value: f64, labels: HashMap<String, String>) -> Self {
        self.values.push((value, labels));
        self
    }

    /// Format as Prometheus text
    pub fn to_prometheus_text(&self) -> String {
        let mut output = String::new();

        // HELP line
        output.push_str(&format!("# HELP {} {}\n", self.name, self.help));
        // TYPE line
        output.push_str(&format!("# TYPE {} {}\n", self.name, self.metric_type));

        // Values
        for (value, labels) in &self.values {
            if labels.is_empty() {
                output.push_str(&format!("{} {}\n", self.name, value));
            } else {
                let label_str: Vec<String> = labels
                    .iter()
                    .map(|(k, v)| format!("{}=\"{}\"", k, v))
                    .collect();
                output.push_str(&format!("{}{{{}}} {}\n", self.name, label_str.join(","), value));
            }
        }

        output
    }
}

/// Distributed observability collector
pub struct ObservabilityCollector {
    /// Controller ID
    controller_id: ControllerId,
    /// Start time
    start_time: SystemTime,
    /// Cluster ID
    cluster_id: String,
}

impl ObservabilityCollector {
    /// Create a new observability collector
    pub fn new(controller_id: ControllerId, cluster_id: String) -> Self {
        Self {
            controller_id,
            start_time: SystemTime::now(),
            cluster_id,
        }
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time
            .elapsed()
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Build health response
    pub fn build_health_response(
        &self,
        role: ControllerRole,
        checks: HashMap<String, ComponentHealth>,
    ) -> HealthResponse {
        // Determine overall status from component checks
        let status = checks
            .values()
            .map(|c| c.status)
            .fold(HealthStatus::Healthy, |acc, s| {
                match (acc, s) {
                    (HealthStatus::Unhealthy, _) | (_, HealthStatus::Unhealthy) => {
                        HealthStatus::Unhealthy
                    }
                    (HealthStatus::Degraded, _) | (_, HealthStatus::Degraded) => {
                        HealthStatus::Degraded
                    }
                    _ => HealthStatus::Healthy,
                }
            });

        HealthResponse {
            status,
            controller_id: self.controller_id.0.clone(),
            role: format!("{:?}", role),
            uptime_seconds: self.uptime_seconds(),
            timestamp: now_ms(),
            checks,
        }
    }

    /// Build cluster status response
    pub fn build_cluster_status(
        &self,
        leader_id: Option<ControllerId>,
        local_role: ControllerRole,
        controllers: Vec<ControllerStatusInfo>,
        current_term: u64,
    ) -> ClusterStatusResponse {
        let total_controllers = controllers.len();
        let healthy_controllers = controllers
            .iter()
            .filter(|c| c.health == "healthy")
            .count();

        let quorum = total_controllers / 2 + 1;
        let has_quorum = healthy_controllers >= quorum;

        ClusterStatusResponse {
            cluster_id: self.cluster_id.clone(),
            leader_id: leader_id.map(|id| id.0),
            local_role: format!("{:?}", local_role),
            total_controllers,
            healthy_controllers,
            has_quorum,
            current_term,
            controllers,
        }
    }

    /// Build Prometheus metrics
    pub fn build_prometheus_metrics(
        &self,
        role: ControllerRole,
        load: &ControllerLoad,
        work_unit_stats: &WorkUnitStats,
        peer_count: usize,
    ) -> Vec<PrometheusMetric> {
        let mut labels = HashMap::new();
        labels.insert("controller".to_string(), self.controller_id.0.clone());
        labels.insert("cluster".to_string(), self.cluster_id.clone());

        vec![
            // Controller metrics
            PrometheusMetric::gauge(
                "rustible_controller_up",
                "Whether the controller is up (1) or down (0)",
            )
            .with_labeled_value(1.0, labels.clone()),
            PrometheusMetric::gauge(
                "rustible_controller_is_leader",
                "Whether this controller is the leader",
            )
            .with_labeled_value(
                if role == ControllerRole::Leader { 1.0 } else { 0.0 },
                labels.clone(),
            ),
            PrometheusMetric::gauge(
                "rustible_controller_uptime_seconds",
                "Controller uptime in seconds",
            )
            .with_labeled_value(self.uptime_seconds() as f64, labels.clone()),
            // Load metrics
            PrometheusMetric::gauge(
                "rustible_controller_load_score",
                "Controller load score (0.0 - 1.0)",
            )
            .with_labeled_value(load.load_score(), labels.clone()),
            PrometheusMetric::gauge(
                "rustible_controller_cpu_usage",
                "Controller CPU usage percentage",
            )
            .with_labeled_value(load.cpu_usage as f64, labels.clone()),
            PrometheusMetric::gauge(
                "rustible_controller_memory_usage",
                "Controller memory usage percentage",
            )
            .with_labeled_value(load.memory_usage as f64, labels.clone()),
            PrometheusMetric::gauge(
                "rustible_controller_active_connections",
                "Number of active host connections",
            )
            .with_labeled_value(load.active_connections as f64, labels.clone()),
            PrometheusMetric::gauge(
                "rustible_controller_queue_depth",
                "Number of pending work units in queue",
            )
            .with_labeled_value(load.queue_depth as f64, labels.clone()),
            // Work unit metrics
            PrometheusMetric::gauge(
                "rustible_work_units_active",
                "Number of currently active work units",
            )
            .with_labeled_value(load.active_work_units as f64, labels.clone()),
            PrometheusMetric::counter(
                "rustible_work_units_total",
                "Total number of work units processed",
            )
            .with_labeled_value(work_unit_stats.total as f64, labels.clone()),
            PrometheusMetric::counter(
                "rustible_work_units_completed",
                "Number of completed work units",
            )
            .with_labeled_value(work_unit_stats.completed as f64, labels.clone()),
            PrometheusMetric::counter("rustible_work_units_failed", "Number of failed work units")
                .with_labeled_value(work_unit_stats.failed as f64, labels.clone()),
            // Cluster metrics
            PrometheusMetric::gauge("rustible_cluster_peers", "Number of known cluster peers")
                .with_labeled_value(peer_count as f64, labels.clone()),
        ]
    }

    /// Format metrics as Prometheus text exposition format
    pub fn format_prometheus_text(&self, metrics: &[PrometheusMetric]) -> String {
        metrics
            .iter()
            .map(|m| m.to_prometheus_text())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Work unit statistics
#[derive(Debug, Clone, Default)]
pub struct WorkUnitStats {
    /// Total work units processed
    pub total: usize,
    /// Pending work units
    pub pending: usize,
    /// Running work units
    pub running: usize,
    /// Completed work units
    pub completed: usize,
    /// Failed work units
    pub failed: usize,
}

/// Ready check response for Kubernetes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyResponse {
    /// Whether the controller is ready
    pub ready: bool,
    /// Reason if not ready
    pub reason: Option<String>,
}

impl ReadyResponse {
    /// Create a ready response
    pub fn ready() -> Self {
        Self {
            ready: true,
            reason: None,
        }
    }

    /// Create a not ready response
    pub fn not_ready(reason: impl Into<String>) -> Self {
        Self {
            ready: false,
            reason: Some(reason.into()),
        }
    }
}

/// Live check response for Kubernetes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveResponse {
    /// Whether the controller is alive
    pub alive: bool,
}

impl LiveResponse {
    /// Create an alive response
    pub fn alive() -> Self {
        Self { alive: true }
    }
}

/// Helper function to get current time in milliseconds
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response() {
        let collector = ObservabilityCollector::new(
            ControllerId::new("test-ctrl"),
            "test-cluster".to_string(),
        );

        let mut checks = HashMap::new();
        checks.insert("raft".to_string(), ComponentHealth::healthy());
        checks.insert("storage".to_string(), ComponentHealth::healthy());

        let response = collector.build_health_response(ControllerRole::Leader, checks);

        assert_eq!(response.status, HealthStatus::Healthy);
        assert_eq!(response.controller_id, "test-ctrl");
        assert_eq!(response.role, "Leader");
    }

    #[test]
    fn test_degraded_health() {
        let collector = ObservabilityCollector::new(
            ControllerId::new("test-ctrl"),
            "test-cluster".to_string(),
        );

        let mut checks = HashMap::new();
        checks.insert("raft".to_string(), ComponentHealth::healthy());
        checks.insert(
            "storage".to_string(),
            ComponentHealth::degraded("High latency"),
        );

        let response = collector.build_health_response(ControllerRole::Follower, checks);

        assert_eq!(response.status, HealthStatus::Degraded);
    }

    #[test]
    fn test_prometheus_metric_format() {
        let metric = PrometheusMetric::gauge("test_metric", "A test metric").with_value(42.0);

        let text = metric.to_prometheus_text();
        assert!(text.contains("# HELP test_metric A test metric"));
        assert!(text.contains("# TYPE test_metric gauge"));
        assert!(text.contains("test_metric 42"));
    }

    #[test]
    fn test_prometheus_metric_with_labels() {
        let mut labels = HashMap::new();
        labels.insert("instance".to_string(), "localhost".to_string());
        labels.insert("job".to_string(), "test".to_string());

        let metric =
            PrometheusMetric::counter("requests_total", "Total requests").with_labeled_value(100.0, labels);

        let text = metric.to_prometheus_text();
        assert!(text.contains("# TYPE requests_total counter"));
        assert!(text.contains("instance=\"localhost\""));
        assert!(text.contains("job=\"test\""));
    }

    #[test]
    fn test_cluster_status_quorum() {
        let collector = ObservabilityCollector::new(
            ControllerId::new("test-ctrl"),
            "test-cluster".to_string(),
        );

        let controllers = vec![
            ControllerStatusInfo {
                id: "ctrl-1".to_string(),
                address: "127.0.0.1:9000".to_string(),
                role: "Leader".to_string(),
                health: "healthy".to_string(),
                region: None,
                load: LoadMetrics::default(),
                last_heartbeat_age_ms: Some(100),
            },
            ControllerStatusInfo {
                id: "ctrl-2".to_string(),
                address: "127.0.0.1:9001".to_string(),
                role: "Follower".to_string(),
                health: "healthy".to_string(),
                region: None,
                load: LoadMetrics::default(),
                last_heartbeat_age_ms: Some(150),
            },
            ControllerStatusInfo {
                id: "ctrl-3".to_string(),
                address: "127.0.0.1:9002".to_string(),
                role: "Follower".to_string(),
                health: "unhealthy".to_string(),
                region: None,
                load: LoadMetrics::default(),
                last_heartbeat_age_ms: None,
            },
        ];

        let response = collector.build_cluster_status(
            Some(ControllerId::new("ctrl-1")),
            ControllerRole::Leader,
            controllers,
            5,
        );

        assert_eq!(response.total_controllers, 3);
        assert_eq!(response.healthy_controllers, 2);
        assert!(response.has_quorum); // 2 out of 3 is quorum
        assert_eq!(response.current_term, 5);
    }

    #[test]
    fn test_load_metrics_from_controller_load() {
        let load = ControllerLoad {
            active_work_units: 10,
            active_connections: 50,
            cpu_usage: 45.0,
            memory_usage: 60.0,
            bandwidth_usage: 30.0,
            avg_latency_ms: 100,
            queue_depth: 5,
            capacity: 100,
        };

        let metrics = LoadMetrics::from(&load);

        assert_eq!(metrics.active_work_units, 10);
        assert_eq!(metrics.active_connections, 50);
        assert_eq!(metrics.cpu_usage, 45.0);
        assert!(metrics.load_score > 0.0);
    }

    #[test]
    fn test_ready_response() {
        let ready = ReadyResponse::ready();
        assert!(ready.ready);
        assert!(ready.reason.is_none());

        let not_ready = ReadyResponse::not_ready("Initializing");
        assert!(!not_ready.ready);
        assert_eq!(not_ready.reason.unwrap(), "Initializing");
    }
}
