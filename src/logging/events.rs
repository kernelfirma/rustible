use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustibleEvent {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,

    pub timestamp_ns: u64,
    pub duration_ns: Option<u64>,

    pub event_name: String,
    pub event_type: String,
    pub severity: String,

    pub operation_name: String,
    pub correlation_id: Option<String>,
    pub attempt_count: Option<u32>,

    pub host_id: String,
    pub host_labels: HashMap<String, String>,
    pub inventory_group: Vec<String>,

    pub user_id: Option<String>,
    pub sudo_user: Option<String>,
    pub authentication_method: Option<String>,
    pub connection_type: Option<String>,

    pub module_name: Option<String>,
    pub task_name: Option<String>,
    pub task_id: Option<String>,
    pub role_name: Option<String>,
    pub playbook_name: Option<String>,

    pub duration_ms: f64,
    pub cpu_time_ms: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub network_bytes_sent: Option<u64>,
    pub network_bytes_received: Option<u64>,

    pub parallel_workers: Option<u32>,
    pub execution_strategy: Option<String>,
    pub check_mode: bool,
    pub diff_mode: bool,

    pub status: String,
    pub changed: bool,
    pub skipped: bool,
    pub failed: bool,

    pub error_code: Option<i32>,
    pub error_type: Option<String>,
    pub error_message: Option<String>,
    pub error_stack_trace: Option<String>,

    pub files_changed: Option<Vec<String>>,
    pub packages_installed: Option<Vec<String>>,
    pub packages_removed: Option<Vec<String>>,
    pub services_started: Option<Vec<String>>,
    pub services_stopped: Option<Vec<String>>,

    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_user: Option<String>,
    pub ssh_auth_method: Option<String>,
    pub ssh_connection_time_ms: Option<f64>,
    pub ssh_handshake_time_ms: Option<f64>,

    pub pool_hits: Option<u32>,
    pub pool_misses: Option<u32>,
    pub pool_size: Option<u32>,
    pub pool_max_size: Option<u32>,

    pub template_path: Option<String>,
    pub template_variables_count: Option<u32>,
    pub template_render_time_ms: Option<f64>,

    pub inventory_file: Option<String>,
    pub inventory_host_count: Option<u32>,
    pub inventory_group_count: Option<u32>,
    pub inventory_vars_count: Option<u32>,

    pub os_type: Option<String>,
    pub os_version: Option<String>,
    pub arch: Option<String>,
    pub rustible_version: Option<String>,

    pub config_file: Option<String>,
    pub config_profile: Option<String>,
    pub feature_flags: Option<Vec<String>>,

    pub timeout_seconds: Option<u32>,
    pub max_retries: Option<u32>,

    pub telemetry_enabled: Option<bool>,
    pub telemetry_sampled: bool,
    pub sampling_reason: Option<String>,

    pub custom_fields: Option<HashMap<String, serde_json::Value>>,
}

impl RustibleEvent {
    pub fn new(
        trace_id: String,
        event_name: String,
        event_type: String,
        severity: String,
        host_id: String,
        status: String,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        Self {
            trace_id,
            span_id: Uuid::new_v4().to_string(),
            parent_span_id: None,
            timestamp_ns: u64::try_from(now).unwrap(),
            duration_ns: None,
            event_name,
            event_type,
            severity,
            operation_name: String::new(),
            correlation_id: None,
            attempt_count: None,
            host_id,
            host_labels: HashMap::new(),
            inventory_group: Vec::new(),
            user_id: None,
            sudo_user: None,
            authentication_method: None,
            connection_type: None,
            module_name: None,
            task_name: None,
            task_id: None,
            role_name: None,
            playbook_name: None,
            duration_ms: 0.0,
            cpu_time_ms: None,
            memory_bytes: None,
            network_bytes_sent: None,
            network_bytes_received: None,
            parallel_workers: None,
            execution_strategy: None,
            check_mode: false,
            diff_mode: false,
            status,
            changed: false,
            skipped: false,
            failed: false,
            error_code: None,
            error_type: None,
            error_message: None,
            error_stack_trace: None,
            files_changed: None,
            packages_installed: None,
            packages_removed: None,
            services_started: None,
            services_stopped: None,
            ssh_host: None,
            ssh_port: None,
            ssh_user: None,
            ssh_auth_method: None,
            ssh_connection_time_ms: None,
            ssh_handshake_time_ms: None,
            pool_hits: None,
            pool_misses: None,
            pool_size: None,
            pool_max_size: None,
            template_path: None,
            template_variables_count: None,
            template_render_time_ms: None,
            inventory_file: None,
            inventory_host_count: None,
            inventory_group_count: None,
            inventory_vars_count: None,
            os_type: None,
            os_version: None,
            arch: None,
            rustible_version: None,
            config_file: None,
            config_profile: None,
            feature_flags: None,
            timeout_seconds: None,
            max_retries: None,
            telemetry_enabled: None,
            telemetry_sampled: false,
            sampling_reason: None,
            custom_fields: None,
        }
    }

    pub fn with_host_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.host_labels = labels;
        self
    }

    pub fn with_inventory_groups(mut self, groups: Vec<String>) -> Self {
        self.inventory_group = groups;
        self
    }

    pub fn with_module(mut self, module: String) -> Self {
        self.module_name = Some(module);
        self
    }

    pub fn with_task(mut self, name: String, id: String) -> Self {
        self.task_name = Some(name);
        self.task_id = Some(id);
        self
    }

    pub fn with_duration(mut self, duration_ns: u64) -> Self {
        self.duration_ns = Some(duration_ns);
        self.duration_ms = duration_ns as f64 / 1_000_000.0;
        self
    }

    pub fn with_result(mut self, changed: bool, failed: bool, skipped: bool) -> Self {
        self.changed = changed;
        self.failed = failed;
        self.skipped = skipped;
        self
    }

    pub fn with_error(mut self, code: i32, error_type: String, message: String) -> Self {
        self.error_code = Some(code);
        self.error_type = Some(error_type);
        self.error_message = Some(message);
        self.failed = true;
        self
    }

    pub fn with_ssh_details(
        mut self,
        host: String,
        port: u16,
        user: String,
        auth_method: String,
    ) -> Self {
        self.ssh_host = Some(host);
        self.ssh_port = Some(port);
        self.ssh_user = Some(user);
        self.ssh_auth_method = Some(auth_method);
        self
    }

    pub fn with_custom_field(mut self, key: String, value: serde_json::Value) -> Self {
        if self.custom_fields.is_none() {
            self.custom_fields = Some(HashMap::new());
        }
        self.custom_fields.as_mut().unwrap().insert(key, value);
        self
    }

    pub fn with_sampling(mut self, sampled: bool, reason: String) -> Self {
        self.telemetry_sampled = sampled;
        self.sampling_reason = Some(reason);
        self
    }
}
