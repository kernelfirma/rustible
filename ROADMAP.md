# Rustible Roadmap

## Vision

Ansible's simplicity + Nix's guarantees + Rust's performance.

## Current State (v0.1-alpha)

**Working:**

- SSH connection pooling (11x faster)
- Module classification system (4 tiers)
- Parallel execution strategies (Linear, Free, HostPinned)
- 50+ native modules
- Python module fallback (FQCN support)
- VM-based test infrastructure

**Stats:** ~2000 tests, 99%+ pass rate

## Roadmap

### v0.2 - Stabilization ✅

- [x] Fix remaining test failures
- [x] Enforce ParallelizationHint in executor
- [x] Add `--plan` flag (execution preview)
- [x] State manifest skeleton

### v0.3 - Nix-Inspired Features ✅

- [x] State hashing/caching (skip unchanged tasks) - `src/state/hashing.rs`
- [x] Drift detection command - `rustible drift` CLI command
- [x] Schema validation at parse time - `src/parser/schema.rs`
- [x] Pipelined SSH - `src/connection/pipelining.rs`

### v0.4 - Performance

- [ ] Lockfile support
- [ ] Transactional checkpoints
- [ ] Native package manager bindings (libapt, librpm)

### v1.0 - Production Ready

- [ ] Dependency graph execution (DAG)
- [ ] Optional agent mode
- [ ] 95%+ Ansible compatibility

## Architecture

### Module Classification

```rust
pub enum ModuleClassification {
    LocalLogic,      // Control node only (debug, set_fact)
    NativeTransport, // Native SSH/SFTP (copy, template)
    RemoteCommand,   // SSH command (service, package)
    PythonFallback,  // Ansible compatibility
}
```

### Execution Strategies

- **Linear**: Task-by-task across hosts (Ansible default)
- **Free**: Maximum parallelism
- **HostPinned**: Dedicated worker per host

## Performance

| Metric              | Ansible    | Rustible | Speedup |
| ------------------- | ---------- | -------- | ------- |
| Connection overhead | Reconnects | Pooled   | 11x     |
| file module         | 80ms       | 8ms      | 10x     |
| copy module         | 120ms      | 15ms     | 8x      |
| command module      | 100ms      | 10ms     | 10x     |

## Comparison

| Feature         | Ansible     | NixOS      | Rustible           |
| --------------- | ----------- | ---------- | ------------------ |
| Speed           | Slow        | Fast       | Fast               |
| Idempotency     | Best effort | Guaranteed | Trait-enforced     |
| Learning curve  | Low         | High       | Low                |
| Reproducibility | Best effort | Perfect    | Lockfile (planned) |
