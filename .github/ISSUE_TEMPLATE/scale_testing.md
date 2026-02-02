#182 [MEDIUM] Large-Scale Testing and Validation (5000+ Hosts)

## Problem Statement
Rustible's performance claims (5-6x faster than Ansible) are based on testing with up to ~100 hosts. There is no validation for large-scale deployments (1000+ hosts), and the architecture's behavior at scale is unknown. This creates risks for enterprise adoption.

## Current State
| Scale | Status | Evidence |
|-------|--------|----------|
| 10 hosts | ✅ Tested | Real SSH tests pass |
| 100 hosts | ⚠️ Partial | Documented but limited real testing |
| 500 hosts | ❌ Not tested | Theoretical only |
| 1000 hosts | ❌ Not tested | Extrapolation from smaller tests |
| 5000+ hosts | ❌ Not tested | Unknown behavior |

## Identified Scaling Risks
| Risk | Impact | Likelihood |
|------|--------|------------|
| **Connection pool exhaustion** | Connection failures | High |
| **Memory exhaustion** | OOM crashes | Medium |
| **File descriptor limits** | Cannot open new connections | High |
| **Network saturation** | Timeouts and failures | Medium |
| **SSH key auth rate limiting** | Connection failures | Medium |
| **Inventory parse slowdown** | Startup delays | Low |
| **Template rendering bottlenecks** | Task execution delays | Medium |

## Comparison to Tested Limits
| Tool | Tested Scale | Production Use |
|------|--------------|----------------|
| **Rustible** | ~100 hosts | Unknown |
| **Ansible** | 1000+ hosts | 10,000+ with Tower |
| **Terraform** | 1000+ resources | Enterprise scale |

## Proposed Implementation

### Phase 1: Scalability Test Suite
```rust
// tests/scalability/
#[cfg(test)]
mod scalability_tests {
    use rustible::*;
    
    #[tokio::test]
    async fn test_100_hosts() {
        // Test with 100 real or simulated hosts
    }
    
    #[tokio::test]
    async fn test_1000_hosts() {
        // Test with 1000 hosts
    }
    
    #[tokio::test]
    async fn test_5000_hosts() {
        // Test with 5000 hosts
    }
}
```

- [ ] Create scalability test framework
- [ ] Implement SSH simulation for large-scale testing
- [ ] Add memory profiling to tests
- [ ] Add performance metrics collection
- [ ] Create test fixtures for 1000/5000 host inventories

### Phase 2: Simulated Environment Testing
```rust
// tests/scalability/mock_ssh.rs
pub struct MockSshServer {
    host_count: usize,
    delay_distribution: DelayDistribution,
    failure_rate: f64,
}

impl MockSshServer {
    pub async fn simulate_host_response(&self) -> Result<CommandResult, SshError> {
        // Simulate realistic SSH behavior
        // Variable latency
        // Occasional failures
        // Resource contention
    }
}
```

- [ ] Build SSH simulation framework
- [ ] Model realistic network conditions (latency, packet loss)
- [ ] Simulate slow hosts and timeouts
- [ ] Test with connection failures and retries
- [ ] Variable host response times

### Phase 3: Resource Limit Testing

#### Connection Pool Limits
```bash
# Test connection pool exhaustion
cargo test --test scalability -- pool_exhaustion_1000_hosts

# Monitor connection pool metrics
cargo test --test scalability -- pool_metrics_5000_hosts
```

- [ ] Test with `max_connections_per_host` limits
- [ ] Test total connection pool limits (50 default)
- [ ] Measure pool contention under load
- [ ] Test pool recovery after exhaustion

#### Memory Usage
```rust
// tests/scalability/memory.rs
#[test]
fn test_memory_scaling() {
    let host_counts = vec![100, 500, 1000, 5000];
    
    for count in host_counts {
        let mem_before = get_memory_usage();
        run_playbook_with_hosts(count);
        let mem_after = get_memory_usage();
        
        let mem_per_host = (mem_after - mem_before) / count;
        println!("{} hosts: {} bytes/host", count, mem_per_host);
        
        // Assert within acceptable bounds
        assert!(mem_per_host < 1_000_000); // <1MB per host
    }
}
```

- [ ] Memory usage per host at different scales
- [ ] Memory leak detection over long runs
- [ ] Memory pressure testing (simulate OOM conditions)
- [ ] Connection memory overhead measurement

#### File Descriptor Limits
```bash
# Test file descriptor usage
ulimit -n 1024  # Simulate restricted environment
cargo test --test scalability -- fd_usage_1000_hosts

ulimit -n 65535  # High limit environment
cargo test --test scalability -- fd_usage_5000_hosts
```

- [ ] File descriptor usage per connection
- [ ] Test with restricted ulimit
- [ ] Proper cleanup verification
- [ ] FD leak detection

### Phase 4: Performance Benchmarks at Scale

#### Fork Scaling Efficiency
```rust
// benches/scalability/fork_scaling.rs
pub fn fork_scaling_benchmark(c: &mut Criterion) {
    let host_counts = vec![10, 100, 500, 1000];
    let fork_counts = vec![10, 50, 100, 200];
    
    for hosts in &host_counts {
        for forks in &fork_counts {
            c.bench_with_input(
                BenchmarkId::new("fork_scaling", format!("{}h_{}f", hosts, forks)),
                &(*hosts, *forks),
                |b, (h, f)| b.iter(|| run_parallel_tasks(*h, *f)),
            );
        }
    }
}
```

- [ ] Benchmark fork scaling at different host counts
- [ ] Identify optimal fork count per scale
- [ ] Measure diminishing returns
- [ ] Network saturation points

#### Latency Distribution
```rust
// Measure task latency distribution
pub async fn measure_latency_distribution(host_count: usize) -> LatencyDistribution {
    let mut latencies = Vec::new();
    
    for host in generate_hosts(host_count) {
        let start = Instant::now();
        execute_task_on_host(&host).await;
        latencies.push(start.elapsed());
    }
    
    LatencyDistribution::from_samples(latencies)
}
```

- [ ] P50, P95, P99 latency at different scales
- [ ] Tail latency analysis
- [ ] Latency correlation with host count
- [ ] Identify stragglers (slow hosts)

### Phase 5: Real Infrastructure Testing

#### Test Environments
- [ ] AWS EC2 test fleet (100/500/1000 instances)
- [ ] Azure VM scale sets
- [ ] GCP compute instances
- [ ] Mixed on-premise + cloud testing

#### Test Scenarios
```yaml
# tests/scalability/scenarios.yml
scenarios:
  - name: simple_command
    description: Execute simple command on all hosts
    playbook: |
      - hosts: all
        tasks:
          - command: echo "hello"
    
  - name: fact_gathering
    description: Gather facts from all hosts
    playbook: |
      - hosts: all
        gather_facts: yes
        tasks: []
    
  - name: file_distribution
    description: Copy file to all hosts
    playbook: |
      - hosts: all
        tasks:
          - copy:
              content: "test"
              dest: /tmp/test.txt
    
  - name: complex_playbook
    description: Multi-task playbook with templates
    playbook: complex_playbook.yml
```

### Phase 6: Optimization Recommendations

Based on test results, create:
- [ ] Capacity planning guide (hosts vs resources)
- [ ] Tuning recommendations per scale
- [ ] Connection pool sizing guidelines
- [ ] Memory requirements calculator
- [ ] Network bandwidth requirements
- [ ] Fork count recommendations

## Documentation Deliverables

### Capacity Planning Guide
```markdown
# Rustible Capacity Planning

## Resource Requirements

| Hosts | RAM | CPU | Network | Connections |
|-------|-----|-----|---------|-------------|
| 100   | 2GB | 2   | 100Mbps | 50          |
| 500   | 8GB | 4   | 500Mbps | 100         |
| 1000  | 16GB| 8   | 1Gbps   | 200         |
| 5000  | 64GB| 16  | 5Gbps   | 500         |

## Configuration Tuning

### Connection Pool
```toml
[connection]
max_connections_per_host = 5  # Increase for 1000+ hosts
max_total_connections = 200   # Increase for 5000+ hosts
pool_timeout = 60             # Increase for slow networks
```

### Forks
- 100 hosts: forks = 20
- 500 hosts: forks = 50  
- 1000 hosts: forks = 100
- 5000 hosts: forks = 200 (requires distributed execution)
```

## Acceptance Criteria
- [ ] Successfully tested with 1000 simulated hosts
- [ ] Successfully tested with 100 real AWS EC2 instances
- [ ] Memory usage documented for each scale point
- [ ] Performance degradation <50% at 1000 hosts vs 100 hosts
- [ ] No resource leaks at any scale
- [ ] Capacity planning guide published
- [ ] Known limitations documented

## Priority
**MEDIUM** - Important for enterprise confidence; can test gradually

## Related
- Issue #176: Distributed execution (needed for 5000+ hosts)
- Issue #168: russh_auth API update (stability at scale)

## Labels
`medium`, `testing`, `scalability`, `performance`, `enterprise`
