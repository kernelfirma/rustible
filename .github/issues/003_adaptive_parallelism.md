# Performance: Implement Adaptive Parallelism for Host Responsiveness

## Problem Statement
Rustible's current parallel execution strategies (Linear, Free, HostPinned) are static and don't adapt to varying host responsiveness. This leads to suboptimal performance in heterogeneous environments where some hosts are slower or have higher latency.

## Current State
- Three execution strategies: Linear, Free, HostPinned
- Fixed `forks` parameter limits parallelism
- No dynamic adjustment based on host performance
- No grouping of hosts by responsiveness

## Proposed Solution

### Phase 1: Host Profiling (v0.1.x)
1. **Track host metrics**
   ```rust
   // src/executor/host_profiler.rs
   pub struct HostProfiler {
       response_times: DashMap<String, RollingAverage>,
       error_rates: DashMap<String, RollingAverage>,
       bandwidth_estimates: DashMap<String, f64>,
   }
   
   impl HostProfiler {
       pub fn record_response(&self, host: &str, duration: Duration) {
           self.response_times
               .entry(host.to_string())
               .or_insert_with(|| RollingAverage::new(10))
               .add(duration.as_secs_f64());
       }
   }
   ```

2. **Host classification**
   - Fast: < 100ms average response time
   - Medium: 100ms - 500ms
   - Slow: > 500ms
   - Unreliable: > 10% error rate

### Phase 2: Adaptive Forks (v0.2.x)
1. **Dynamic fork adjustment**
   ```rust
   // src/executor/adaptive_forks.rs
   pub struct AdaptiveForksManager {
       base_forks: usize,
       max_forks: usize,
       min_forks: usize,
       profiler: Arc<HostProfiler>,
   }
   
   impl AdaptiveForksManager {
       pub fn calculate_optimal_forks(&self, hosts: &[Host]) -> usize {
           let avg_latency = self.profiler.get_average_latency(hosts);
           let target_throughput = 10.0; // tasks per second
           let optimal = (target_throughput / avg_latency).ceil() as usize;
           optimal.clamp(self.min_forks, self.max_forks)
       }
   }
   ```

2. **Host grouping for parallelism**
   ```rust
   // src/executor/host_grouper.rs
   pub enum HostGroup {
       Fast(Vec<Host>),    // Maximize parallelism
       Medium(Vec<Host>),  // Moderate parallelism
       Slow(Vec<Host>),    // Limited parallelism (2-4 forks)
       Unreliable(Vec<Host>), // Serial execution with retries
   }
   ```

3. **Per-group execution**
   - Fast hosts: use `max_forks` (e.g., 50)
   - Medium hosts: use `base_forks` (e.g., 10)
   - Slow hosts: use limited parallelism (e.g., 2-4)
   - Unreliable hosts: execute serially with retry

### Phase 3: Predictive Scaling (v0.3.x)
1. **Machine learning model** (optional, advanced)
   - Train on historical execution data
   - Predict optimal parallelism for new workloads
   - Auto-tune based on playbook characteristics

2. **Workload-aware parallelism**
   - Analyze playbook before execution
   - Identify I/O-bound vs CPU-bound tasks
   - Adjust parallelism strategy accordingly

## Expected Outcomes
- 15-25% additional performance improvement in heterogeneous environments
- Better handling of slow/unreliable hosts
- Reduced timeout errors
- More consistent execution times across varied infrastructure

## Success Criteria
- [ ] Host profiler tracks response times and error rates
- [ ] Adaptive forks manager dynamically adjusts parallelism
- [ ] Host grouping optimized for different host classes
- [ ] 15% performance improvement in heterogeneous benchmarks
- [ ] Reduced timeout rate for slow hosts
- [ ] Configuration options for adaptive behavior

## Implementation Details

### CLI Flags
```bash
rustible run playbook.yml \
  --adaptive-forks \
  --min-forks 2 \
  --max-forks 50 \
  --target-latency 100ms
```

### Configuration
```toml
[executor]
strategy = "adaptive"
adaptive_forks = true
base_forks = 10
max_forks = 50
min_forks = 2
fast_host_threshold = "100ms"
slow_host_threshold = "500ms"
unreliable_host_threshold = "10%"
```

### Metrics
- Per-host response time histogram
- Fork count changes over time
- Throughput by host group
- Error rate by host group

## Related Issues
- #001: Performance Benchmark Suite
- #002: Connection Pooling Optimization
- #004: Execution Strategy Refinement

## Additional Notes
This is a **P1 (High)** feature that builds on existing execution strategies. Should be targeted for v0.2.x release to maximize performance gains.
