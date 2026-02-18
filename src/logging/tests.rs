#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use crate::logging::{should_sample, RustibleEvent};

    #[test]
    fn test_sampling_decision_errors() {
        let decision = should_sample(&tracing::Level::ERROR, "task_execution", Some(100.0));

        assert!(decision.should_log);
        assert_eq!(decision.sampling_reason, "error_or_warn");
    }

    #[test]
    fn test_sampling_decision_warnings() {
        let decision = should_sample(&tracing::Level::WARN, "task_execution", Some(100.0));

        assert!(decision.should_log);
        assert_eq!(decision.sampling_reason, "error_or_warn");
    }

    #[test]
    fn test_sampling_decision_slow() {
        let decision = should_sample(&tracing::Level::INFO, "task_execution", Some(1500.0));

        assert!(decision.should_log);
        assert_eq!(decision.sampling_reason, "slow_operation");
    }

    #[test]
    fn test_sampling_decision_info_within_threshold() {
        let decision = should_sample(&tracing::Level::INFO, "task_execution", Some(500.0));

        if decision.should_log {
            assert_eq!(decision.sampling_reason, "random");
        } else {
            assert_eq!(decision.sampling_reason, "sampled_out");
        }
    }

    #[test]
    fn test_sampling_decision_playbook_random() {
        let decision = should_sample(&tracing::Level::INFO, "playbook_execution", Some(100.0));

        if decision.should_log {
            assert_eq!(decision.sampling_reason, "random");
        } else {
            assert_eq!(decision.sampling_reason, "sampled_out");
        }
    }

    #[test]
    fn test_sampling_decision_task_random() {
        let decision = should_sample(&tracing::Level::INFO, "task_execution", Some(100.0));

        if decision.should_log {
            assert_eq!(decision.sampling_reason, "random");
        } else {
            assert_eq!(decision.sampling_reason, "sampled_out");
        }
    }

    #[test]
    fn test_event_builder_basic() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        );

        assert_eq!(event.trace_id, "trace-123");
        assert_eq!(event.event_name, "test_event");
        assert_eq!(event.host_id, "host-01");
        assert_eq!(event.status, "success");
        assert!(!event.changed);
        assert!(!event.failed);
    }

    #[test]
    fn test_event_builder_with_duration() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_duration(1_000_000_000);

        assert_eq!(event.duration_ns, Some(1_000_000_000));
        assert_eq!(event.duration_ms, 1000.0);
    }

    #[test]
    fn test_event_builder_with_module() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_module("package".to_string());

        assert_eq!(event.module_name, Some("package".to_string()));
    }

    #[test]
    fn test_event_builder_with_task() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_task("Install nginx".to_string(), "task-001".to_string());

        assert_eq!(event.task_name, Some("Install nginx".to_string()));
        assert_eq!(event.task_id, Some("task-001".to_string()));
    }

    #[test]
    fn test_event_builder_with_result() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_result(true, false, false);

        assert!(event.changed);
        assert!(!event.failed);
        assert!(!event.skipped);
    }

    #[test]
    fn test_event_builder_with_error() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_error(1, "timeout".to_string(), "Connection timeout".to_string());

        assert_eq!(event.error_code, Some(1));
        assert_eq!(event.error_type, Some("timeout".to_string()));
        assert_eq!(event.error_message, Some("Connection timeout".to_string()));
        assert!(event.failed);
    }

    #[test]
    fn test_event_builder_with_ssh_details() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_ssh_details(
            "web-01.example.com".to_string(),
            22,
            "ansible".to_string(),
            "key".to_string(),
        );

        assert_eq!(event.ssh_host, Some("web-01.example.com".to_string()));
        assert_eq!(event.ssh_port, Some(22));
        assert_eq!(event.ssh_user, Some("ansible".to_string()));
        assert_eq!(event.ssh_auth_method, Some("key".to_string()));
    }

    #[test]
    fn test_event_builder_with_custom_field() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_custom_field(
            "custom_key".to_string(),
            serde_json::Value::String("custom_value".to_string()),
        )
        .with_custom_field("another_key".to_string(), serde_json::json!(42));

        assert!(event.custom_fields.is_some());
        let fields = event.custom_fields.unwrap();
        assert_eq!(
            fields.get("custom_key"),
            Some(&serde_json::Value::String("custom_value".to_string()))
        );
        assert_eq!(
            fields.get("another_key"),
            Some(&serde_json::Value::Number(42.into()))
        );
    }

    #[test]
    fn test_event_builder_with_sampling() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_sampling(true, "random".to_string());

        assert!(event.telemetry_sampled);
        assert_eq!(event.sampling_reason, Some("random".to_string()));
    }

    #[test]
    fn test_event_json_serialization() {
        let event = RustibleEvent::new(
            "trace-123".to_string(),
            "test_event".to_string(),
            "operation".to_string(),
            "info".to_string(),
            "host-01".to_string(),
            "success".to_string(),
        )
        .with_module("package".to_string())
        .with_duration(1_000_000_000);

        let json = serde_json::to_value(&event).expect("Failed to serialize event");
        assert!(json.is_object());

        let obj = json.as_object().unwrap();
        assert_eq!(obj.get("trace_id").unwrap().as_str(), Some("trace-123"));
        assert_eq!(obj.get("event_name").unwrap().as_str(), Some("test_event"));
        assert_eq!(obj.get("module_name").unwrap().as_str(), Some("package"));
        assert_eq!(obj.get("duration_ms").unwrap().as_f64(), Some(1000.0));
    }
}
