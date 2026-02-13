//! Chaos layer for wrapping connection-like operations.
//!
//! Provides [`ChaosLayer`] which can be placed in front of any connection or
//! service call to inject delays and failures according to configured faults.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::fault::Fault;

/// A chaos layer that evaluates configured faults against each call.
///
/// This struct is designed to sit in front of any operation and decide
/// whether to inject a failure or delay. It tracks a per-instance call
/// count so that faults like [`Fault::FailAfterN`] work correctly.
pub struct ChaosLayer {
    faults: Vec<Fault>,
    call_count: AtomicUsize,
}

impl ChaosLayer {
    /// Creates a new `ChaosLayer` with no faults configured.
    pub fn new() -> Self {
        Self {
            faults: Vec::new(),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Adds a fault to this layer.
    pub fn add_fault(&mut self, fault: Fault) {
        self.faults.push(fault);
    }

    /// Returns the current call count.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    /// Returns the configured faults.
    pub fn faults(&self) -> &[Fault] {
        &self.faults
    }

    /// Evaluates all configured faults and returns an error message if any
    /// fault triggers. The call count is incremented once per invocation.
    ///
    /// Returns `None` if no fault triggered and the operation should proceed.
    pub fn should_fail(&self) -> Option<String> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);

        for fault in &self.faults {
            match fault {
                Fault::Delay { .. } => {
                    // Delays are handled by `apply_delay`; not a failure.
                }
                Fault::RandomFailure { probability } => {
                    let current = (count as f64) * probability;
                    let next = ((count + 1) as f64) * probability;
                    if next.floor() > current.floor() {
                        return Some(format!(
                            "ChaosLayer: RandomFailure triggered (p={probability}, call={count})"
                        ));
                    }
                }
                Fault::FailAfterN { n } => {
                    if count >= *n {
                        return Some(format!(
                            "ChaosLayer: FailAfterN triggered (n={n}, call={count})"
                        ));
                    }
                }
                Fault::NetworkPartition => {
                    return Some("ChaosLayer: NetworkPartition - connection refused".to_string());
                }
                Fault::PacketLoss { rate } => {
                    let current = (count as f64) * rate;
                    let next = ((count + 1) as f64) * rate;
                    if next.floor() > current.floor() {
                        return Some(format!(
                            "ChaosLayer: PacketLoss triggered (rate={rate}, call={count})"
                        ));
                    }
                }
                Fault::ResourceExhaustion => {
                    return Some(
                        "ChaosLayer: ResourceExhaustion - no resources available".to_string(),
                    );
                }
            }
        }

        None
    }

    /// If any [`Fault::Delay`] is configured, sleeps for the total delay
    /// duration asynchronously. Returns the total delay applied in
    /// milliseconds.
    pub async fn apply_delay(&self) -> u64 {
        let total_ms: u64 = self
            .faults
            .iter()
            .filter_map(|f| match f {
                Fault::Delay { ms } => Some(*ms),
                _ => None,
            })
            .sum();

        if total_ms > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(total_ms)).await;
        }

        total_ms
    }
}

impl Default for ChaosLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chaos_layer_no_faults_passes() {
        let layer = ChaosLayer::new();
        assert!(layer.should_fail().is_none());
        assert!(layer.should_fail().is_none());
        assert_eq!(layer.call_count(), 2);
    }

    #[test]
    fn test_chaos_layer_fail_after_n() {
        let mut layer = ChaosLayer::new();
        layer.add_fault(Fault::FailAfterN { n: 2 });

        // Calls 0 and 1 should pass.
        assert!(layer.should_fail().is_none());
        assert!(layer.should_fail().is_none());

        // Call 2 should fail.
        let err = layer.should_fail();
        assert!(err.is_some());
        assert!(err.unwrap().contains("FailAfterN"));
    }

    #[test]
    fn test_chaos_layer_network_partition() {
        let mut layer = ChaosLayer::new();
        layer.add_fault(Fault::NetworkPartition);

        let err = layer.should_fail();
        assert!(err.is_some());
        assert!(err.unwrap().contains("NetworkPartition"));
    }

    #[tokio::test]
    async fn test_chaos_layer_apply_delay() {
        let mut layer = ChaosLayer::new();
        layer.add_fault(Fault::Delay { ms: 10 });
        layer.add_fault(Fault::Delay { ms: 5 });

        let total = layer.apply_delay().await;
        assert_eq!(total, 15);
    }

    #[test]
    fn test_chaos_layer_default() {
        let layer = ChaosLayer::default();
        assert!(layer.faults().is_empty());
        assert_eq!(layer.call_count(), 0);
    }
}
