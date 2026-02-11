//! Reliability scorecards and regression gates.
//!
//! Provides [`ReliabilityScorecard`] for collecting chaos-test scenario results
//! and [`RegressionGate`] for enforcing minimum pass rates and required
//! scenario categories.

/// Category of a chaos-test scenario.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScenarioCategory {
    /// Tests around connection establishment and reconnection.
    ConnectionResilience,
    /// Tests that verify state can be recovered after a failure.
    StateRecovery,
    /// Tests around concurrent access and lock contention.
    LockContention,
    /// Tests for network partition tolerance.
    PartitionTolerance,
    /// Tests that verify graceful degradation under stress.
    GracefulDegradation,
}

impl std::fmt::Display for ScenarioCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionResilience => write!(f, "ConnectionResilience"),
            Self::StateRecovery => write!(f, "StateRecovery"),
            Self::LockContention => write!(f, "LockContention"),
            Self::PartitionTolerance => write!(f, "PartitionTolerance"),
            Self::GracefulDegradation => write!(f, "GracefulDegradation"),
        }
    }
}

/// The outcome of a single chaos-test scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Name of the scenario.
    pub name: String,
    /// Category this scenario belongs to.
    pub category: ScenarioCategory,
    /// Whether the scenario passed.
    pub passed: bool,
    /// How long the scenario took to run, in milliseconds.
    pub duration_ms: u64,
    /// Optional human-readable detail or error message.
    pub details: Option<String>,
}

/// A scorecard that collects scenario results and computes aggregate metrics.
#[derive(Debug, Default)]
pub struct ReliabilityScorecard {
    scenarios: Vec<ScenarioResult>,
}

impl ReliabilityScorecard {
    /// Creates a new, empty scorecard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a scenario result to the scorecard.
    pub fn add_result(&mut self, result: ScenarioResult) {
        self.scenarios.push(result);
    }

    /// Returns the fraction of scenarios that passed (0.0..=1.0).
    ///
    /// Returns 0.0 if no scenarios have been recorded.
    pub fn pass_rate(&self) -> f64 {
        if self.scenarios.is_empty() {
            return 0.0;
        }
        let passed = self.scenarios.iter().filter(|s| s.passed).count();
        passed as f64 / self.scenarios.len() as f64
    }

    /// Checks whether this scorecard satisfies the given regression gate.
    ///
    /// A gate passes when:
    /// 1. The overall pass rate meets or exceeds `gate.min_pass_rate`.
    /// 2. Every required category has at least one passing scenario.
    pub fn check_gate(&self, gate: &RegressionGate) -> bool {
        if self.pass_rate() < gate.min_pass_rate {
            return false;
        }

        for required in &gate.required_categories {
            let has_passing = self
                .scenarios
                .iter()
                .any(|s| &s.category == required && s.passed);
            if !has_passing {
                return false;
            }
        }

        true
    }

    /// Serializes the scorecard to a JSON string.
    pub fn to_json(&self) -> String {
        let scenarios: Vec<serde_json::Value> = self
            .scenarios
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "category": s.category.to_string(),
                    "passed": s.passed,
                    "duration_ms": s.duration_ms,
                    "details": s.details,
                })
            })
            .collect();

        serde_json::json!({
            "total": self.scenarios.len(),
            "passed": self.scenarios.iter().filter(|s| s.passed).count(),
            "pass_rate": self.pass_rate(),
            "scenarios": scenarios,
        })
        .to_string()
    }

    /// Returns a human-readable summary of the scorecard.
    pub fn summary(&self) -> String {
        let total = self.scenarios.len();
        let passed = self.scenarios.iter().filter(|s| s.passed).count();
        let failed = total - passed;
        let rate = if total > 0 {
            self.pass_rate() * 100.0
        } else {
            0.0
        };

        let mut lines = vec![format!(
            "Reliability Scorecard: {passed}/{total} passed ({rate:.1}%)"
        )];

        if failed > 0 {
            lines.push("  Failed scenarios:".to_string());
            for s in self.scenarios.iter().filter(|s| !s.passed) {
                let detail = s
                    .details
                    .as_deref()
                    .unwrap_or("no details");
                lines.push(format!(
                    "    - {} [{}]: {}",
                    s.name, s.category, detail
                ));
            }
        }

        lines.join("\n")
    }

    /// Returns a reference to the collected scenarios.
    pub fn scenarios(&self) -> &[ScenarioResult] {
        &self.scenarios
    }
}

/// A regression gate that defines minimum acceptance criteria.
#[derive(Debug, Clone)]
pub struct RegressionGate {
    /// Minimum overall pass rate required (0.0..=1.0).
    pub min_pass_rate: f64,
    /// Categories that must have at least one passing scenario.
    pub required_categories: Vec<ScenarioCategory>,
}

impl RegressionGate {
    /// Creates a new regression gate.
    pub fn new(
        min_pass_rate: f64,
        required_categories: Vec<ScenarioCategory>,
    ) -> Self {
        Self {
            min_pass_rate,
            required_categories,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(
        name: &str,
        category: ScenarioCategory,
        passed: bool,
    ) -> ScenarioResult {
        ScenarioResult {
            name: name.to_string(),
            category,
            passed,
            duration_ms: 100,
            details: if passed {
                None
            } else {
                Some("simulated failure".to_string())
            },
        }
    }

    #[test]
    fn test_pass_rate_empty() {
        let sc = ReliabilityScorecard::new();
        assert_eq!(sc.pass_rate(), 0.0);
    }

    #[test]
    fn test_pass_rate_mixed() {
        let mut sc = ReliabilityScorecard::new();
        sc.add_result(make_result(
            "conn-retry",
            ScenarioCategory::ConnectionResilience,
            true,
        ));
        sc.add_result(make_result(
            "state-recover",
            ScenarioCategory::StateRecovery,
            true,
        ));
        sc.add_result(make_result(
            "partition-fail",
            ScenarioCategory::PartitionTolerance,
            false,
        ));

        let rate = sc.pass_rate();
        // 2 out of 3
        assert!((rate - 2.0 / 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_check_gate_passes() {
        let mut sc = ReliabilityScorecard::new();
        sc.add_result(make_result(
            "conn-retry",
            ScenarioCategory::ConnectionResilience,
            true,
        ));
        sc.add_result(make_result(
            "state-recover",
            ScenarioCategory::StateRecovery,
            true,
        ));

        let gate = RegressionGate::new(
            0.5,
            vec![ScenarioCategory::ConnectionResilience],
        );

        assert!(sc.check_gate(&gate));
    }

    #[test]
    fn test_check_gate_fails_missing_category() {
        let mut sc = ReliabilityScorecard::new();
        sc.add_result(make_result(
            "conn-retry",
            ScenarioCategory::ConnectionResilience,
            true,
        ));

        let gate = RegressionGate::new(
            0.5,
            vec![
                ScenarioCategory::ConnectionResilience,
                ScenarioCategory::PartitionTolerance,
            ],
        );

        // PartitionTolerance has no passing scenario.
        assert!(!sc.check_gate(&gate));
    }

    #[test]
    fn test_check_gate_fails_low_pass_rate() {
        let mut sc = ReliabilityScorecard::new();
        sc.add_result(make_result(
            "a",
            ScenarioCategory::ConnectionResilience,
            true,
        ));
        sc.add_result(make_result(
            "b",
            ScenarioCategory::StateRecovery,
            false,
        ));
        sc.add_result(make_result(
            "c",
            ScenarioCategory::LockContention,
            false,
        ));
        sc.add_result(make_result(
            "d",
            ScenarioCategory::PartitionTolerance,
            false,
        ));

        let gate = RegressionGate::new(0.5, vec![]);
        // pass rate is 0.25, below 0.5
        assert!(!sc.check_gate(&gate));
    }

    #[test]
    fn test_to_json_contains_fields() {
        let mut sc = ReliabilityScorecard::new();
        sc.add_result(make_result(
            "test-scenario",
            ScenarioCategory::GracefulDegradation,
            true,
        ));

        let json = sc.to_json();
        assert!(json.contains("\"total\":1"));
        assert!(json.contains("\"passed\":1"));
        assert!(json.contains("test-scenario"));
        assert!(json.contains("GracefulDegradation"));
    }

    #[test]
    fn test_summary_format() {
        let mut sc = ReliabilityScorecard::new();
        sc.add_result(make_result(
            "ok-scenario",
            ScenarioCategory::ConnectionResilience,
            true,
        ));
        sc.add_result(make_result(
            "fail-scenario",
            ScenarioCategory::LockContention,
            false,
        ));

        let summary = sc.summary();
        assert!(summary.contains("1/2 passed"));
        assert!(summary.contains("fail-scenario"));
        assert!(summary.contains("simulated failure"));
    }
}
