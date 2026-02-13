//! Fault definitions and injector implementations.
//!
//! Provides an enum of injectable faults and traits/structs for applying them.

use std::sync::atomic::{AtomicUsize, Ordering};

/// A fault that can be injected into an operation.
#[derive(Debug, Clone)]
pub enum Fault {
    /// Adds a fixed delay (in milliseconds) before the operation proceeds.
    Delay { ms: u64 },
    /// Fails with the given probability (0.0 = never, 1.0 = always).
    RandomFailure { probability: f64 },
    /// Succeeds for the first `n` calls, then fails on every subsequent call.
    FailAfterN { n: usize },
    /// Simulates a network partition (immediate connection refusal).
    NetworkPartition,
    /// Drops a fraction of packets/operations (0.0 = none, 1.0 = all).
    PacketLoss { rate: f64 },
    /// Simulates resource exhaustion (e.g., out of memory / file descriptors).
    ResourceExhaustion,
}

/// Trait for injecting faults into operations.
pub trait FaultInjector: Send + Sync {
    /// Attempt to inject the given fault.
    ///
    /// Returns `Ok(())` if the operation should proceed normally, or
    /// `Err(message)` if the fault triggered and the operation should fail.
    fn inject(&self, fault: &Fault) -> Result<(), String>;

    /// A human-readable name for this injector.
    fn name(&self) -> &str;
}

/// A simple fault injector that evaluates faults using deterministic logic
/// where possible, and probability-based logic otherwise.
pub struct SimpleFaultInjector {
    call_count: AtomicUsize,
}

impl SimpleFaultInjector {
    /// Creates a new `SimpleFaultInjector` with a zero call count.
    pub fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }

    /// Returns the current call count.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    /// Resets the call count to zero.
    pub fn reset(&self) {
        self.call_count.store(0, Ordering::SeqCst);
    }
}

impl Default for SimpleFaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl FaultInjector for SimpleFaultInjector {
    fn inject(&self, fault: &Fault) -> Result<(), String> {
        let count = self.call_count.fetch_add(1, Ordering::SeqCst);

        match fault {
            Fault::Delay { .. } => {
                // Delay is not a failure condition; it is handled externally
                // (e.g., by the ChaosLayer async path). Signal success here.
                Ok(())
            }
            Fault::RandomFailure { probability } => {
                // Deterministic approximation: fail if the fractional part of
                // (count * probability) crosses an integer boundary.
                let current = (count as f64) * probability;
                let next = ((count + 1) as f64) * probability;
                if next.floor() > current.floor() {
                    Err(format!(
                        "RandomFailure triggered (probability={probability}, call={count})"
                    ))
                } else {
                    Ok(())
                }
            }
            Fault::FailAfterN { n } => {
                if count >= *n {
                    Err(format!("FailAfterN triggered (n={n}, call={count})"))
                } else {
                    Ok(())
                }
            }
            Fault::NetworkPartition => Err("NetworkPartition: connection refused".to_string()),
            Fault::PacketLoss { rate } => {
                let current = (count as f64) * rate;
                let next = ((count + 1) as f64) * rate;
                if next.floor() > current.floor() {
                    Err(format!("PacketLoss triggered (rate={rate}, call={count})"))
                } else {
                    Ok(())
                }
            }
            Fault::ResourceExhaustion => {
                Err("ResourceExhaustion: no resources available".to_string())
            }
        }
    }

    fn name(&self) -> &str {
        "SimpleFaultInjector"
    }
}

/// A composite injector that applies multiple inner injectors in sequence.
///
/// If any inner injector returns an error the composite short-circuits and
/// returns that error.
pub struct CompositeFaultInjector {
    injectors: Vec<Box<dyn FaultInjector>>,
}

impl CompositeFaultInjector {
    /// Creates a new composite injector from the given list.
    pub fn new(injectors: Vec<Box<dyn FaultInjector>>) -> Self {
        Self { injectors }
    }

    /// Adds an injector to the end of the chain.
    pub fn add(&mut self, injector: Box<dyn FaultInjector>) {
        self.injectors.push(injector);
    }

    /// Returns the number of inner injectors.
    pub fn len(&self) -> usize {
        self.injectors.len()
    }

    /// Returns `true` if there are no inner injectors.
    pub fn is_empty(&self) -> bool {
        self.injectors.is_empty()
    }
}

impl FaultInjector for CompositeFaultInjector {
    fn inject(&self, fault: &Fault) -> Result<(), String> {
        for injector in &self.injectors {
            injector.inject(fault)?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "CompositeFaultInjector"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_injector_fail_after_n() {
        let injector = SimpleFaultInjector::new();
        let fault = Fault::FailAfterN { n: 3 };

        // First 3 calls succeed (call indices 0, 1, 2).
        assert!(injector.inject(&fault).is_ok());
        assert!(injector.inject(&fault).is_ok());
        assert!(injector.inject(&fault).is_ok());

        // Fourth call (index 3) fails.
        assert!(injector.inject(&fault).is_err());
        assert!(injector.inject(&fault).is_err());
    }

    #[test]
    fn test_simple_injector_network_partition_always_fails() {
        let injector = SimpleFaultInjector::new();
        let fault = Fault::NetworkPartition;

        for _ in 0..5 {
            let result = injector.inject(&fault);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("NetworkPartition"));
        }
    }

    #[test]
    fn test_simple_injector_resource_exhaustion() {
        let injector = SimpleFaultInjector::new();
        let fault = Fault::ResourceExhaustion;

        let result = injector.inject(&fault);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ResourceExhaustion"));
    }

    #[test]
    fn test_simple_injector_delay_does_not_fail() {
        let injector = SimpleFaultInjector::new();
        let fault = Fault::Delay { ms: 500 };

        for _ in 0..10 {
            assert!(injector.inject(&fault).is_ok());
        }
    }

    #[test]
    fn test_composite_injector_short_circuits() {
        let ok_injector = SimpleFaultInjector::new();
        let fail_injector = SimpleFaultInjector::new();

        let composite =
            CompositeFaultInjector::new(vec![Box::new(ok_injector), Box::new(fail_injector)]);

        // NetworkPartition always fails, so the composite should fail
        // even though the first injector would pass for Delay.
        let fault = Fault::NetworkPartition;
        assert!(composite.inject(&fault).is_err());
    }

    #[test]
    fn test_composite_injector_all_pass() {
        let a = SimpleFaultInjector::new();
        let b = SimpleFaultInjector::new();

        let composite = CompositeFaultInjector::new(vec![Box::new(a), Box::new(b)]);

        let fault = Fault::Delay { ms: 100 };
        assert!(composite.inject(&fault).is_ok());
        assert_eq!(composite.len(), 2);
        assert!(!composite.is_empty());
    }

    #[test]
    fn test_simple_injector_random_failure_deterministic() {
        // With probability 0.5, roughly every other call should fail.
        let injector = SimpleFaultInjector::new();
        let fault = Fault::RandomFailure { probability: 0.5 };

        let mut failures = 0;
        let total = 10;
        for _ in 0..total {
            if injector.inject(&fault).is_err() {
                failures += 1;
            }
        }

        // With our deterministic approach, exactly 5 out of 10 should fail.
        assert_eq!(failures, 5);
    }

    #[test]
    fn test_simple_injector_packet_loss() {
        let injector = SimpleFaultInjector::new();
        let fault = Fault::PacketLoss { rate: 0.25 };

        let mut losses = 0;
        let total = 8;
        for _ in 0..total {
            if injector.inject(&fault).is_err() {
                losses += 1;
            }
        }

        // With rate 0.25 over 8 calls, expect exactly 2 losses.
        assert_eq!(losses, 2);
    }
}
