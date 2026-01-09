#[cfg(test)]
mod tests {
    use super::super::should_sample;

    #[test]
    fn test_sampling_decision_error() {
        let decision = should_sample(&tracing::Level::ERROR, "task_execution", None);

        assert!(decision.should_log);
        assert_eq!(decision.sampling_reason, "error_or_warn");
    }

    #[test]
    fn test_sampling_decision_warn() {
        let decision = should_sample(&tracing::Level::WARN, "task_execution", None);

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
        // This test is flaky because of random sampling in should_sample
        // We iterate enough times to likely hit the !should_log case or we should
        // mock random. But since we can't easily mock rand::random here,
        // we will just assert that if it IS logged, the reason is "random".
        // OR, better, we pass a duration that is NOT slow, and event name that is NOT sampled.

        let decision = should_sample(&tracing::Level::INFO, "other_event", Some(500.0));

        assert!(!decision.should_log);
        assert_eq!(decision.sampling_reason, "sampled_out");
    }
}
