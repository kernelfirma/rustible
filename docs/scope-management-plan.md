# Rustible Scope Management Plan

## Executive Summary

This document outlines a structured approach to managing scope creep in the Rustible project. The goal is to focus development effort on core stability while maintaining a clear path for future expansion.

---

## Phase 1: Feature Categorization (Immediate)

### Core Features (Always Enabled)
These are the essential features that make Rustible work as an Ansible alternative:

| Feature | Status | Priority |
|---------|--------|----------|
| `russh` | ✅ Working | P0 - Essential |
| `local` | ✅ Working | P0 - Essential |
| `docker` | ⚠️ Partial (stubs present) | P1 - Polish |

### Extended Features (Optional but Working)
These features are functional but can be disabled:

| Feature | Status | Priority |
|---------|--------|----------|
| `kubernetes` | ✅ Working | P1 - Important |
| `aws` | ✅ Working | P1 - Important |
| `api` | ✅ Working | P2 - Nice to have |

### Experimental/Stubs (Require Major Work)
These features are essentially placeholders and need significant development:

| Feature | Status | Recommendation |
|---------|--------|----------------|
| `azure` | ❌ Stub only | Remove or hide better |
| `gcp` | ❌ Stub only | Remove or hide better |
| `database` | ❌ Stub only | Remove or hide better |
| `winrm` | ❌ Stub only | Remove or hide better |
| `reqwest` | ❌ Stub only | Remove or hide better |
| `distributed` | ⚠️ Partial | Move to separate crate |
| `provisioning` | ⚠️ AWS only | Keep but document limits |

---

## Phase 2: Feature Flag Simplification

### Current Problem

```toml
# Current: Too many combinations
full = ["russh", "local", "ssh2-backend", "docker", "kubernetes"]
full-cloud = ["full", "aws", "azure", "gcp", "experimental"]
full-aws = ["full", "aws"]
full-provisioning = ["full-aws", "provisioning"]
pure-rust = ["russh", "local"]
```

### Proposed Simplification

```toml
# Proposed: Clear, minimal combinations
default = ["russh", "local", "docker"]

# Extended feature sets
cloud = ["aws"]  # Add more as they mature
kubernetes = ["dep:kube", "dep:k8s-openapi"]

# Legacy/compatibility
ssh2-backend = ["dep:ssh2"]

# Everything that actually works
complete = ["russh", "local", "docker", "aws", "kubernetes", "api"]
```

**Remove:** `full-cloud`, `full-aws`, `full-provisioning`, `pure-rust`, `experimental`

---

## Phase 3: Workspace Separation (Recommended)

As the project grows, consider splitting into a workspace:

```
rustible/
├── Cargo.toml                    # Workspace manifest
├── crates/
│   ├── rustible-core/            # Essential execution engine
│   ├── rustible-cli/             # CLI application
│   ├── rustible-modules/         # Built-in modules
│   ├── rustible-ssh/             # SSH connection backends
│   └── rustible-provisioning/    # Terraform-like (future)
└── docs/
```

### Benefits

1. **Faster compile times** - Changes in one crate don't rebuild everything
2. **Clearer dependencies** - Each crate has explicit deps
3. **Easier testing** - Test core without cloud features
4. **Versioning** - Different crates can version independently
5. **Contribution** - Easier for contributors to understand boundaries

---

## Phase 4: Stub Cleanup Strategy

### Option A: Remove Stubs (Recommended for Alpha)

Remove these modules entirely:
- `azure` cloud modules
- `gcp` cloud modules  
- `database` modules (mysql, postgresql)
- `winrm` connection support
- `reqwest` feature flag

**Pros:**
- Cleaner codebase
- Faster compile times
- No confusion about what works

**Cons:**
- Lose the "placeholder" reminder
- Need to re-implement later

### Option B: Consolidate Stubs

Create a single `experimental` module that contains all stubs:

```rust
// src/experimental/mod.rs
pub mod cloud {
    pub mod azure { /* stub */ }
    pub mod gcp { /* stub */ }
}
pub mod database { /* stub */ }
pub mod winrm { /* stub */ }
```

**Pros:**
- Keeps placeholders visible
- Single feature flag: `experimental`

**Cons:**
- Still compiles dead code
- Can confuse users

### Recommendation: Option A

Remove stubs now. Use Git history to recover them later. Create GitHub issues to track planned features.

---

## Phase 5: Development Workflow

### Branch Strategy

```
main          # Stable, core features only
├── develop   # Integration branch
├── feature/  # Individual features
└── release/  # Release preparation
```

### Feature Gates for New Work

1. **All new features** start as separate crates or behind `unstable-*` flags
2. **Feature promotion criteria:**
   - Complete implementation (no stubs)
   - Tests passing
   - Documentation complete
   - Reviewed and approved
3. **Only then** move to main feature flag

### Code Quality Gates

Add to CI:

```yaml
# .github/workflows/scope-check.yml
- name: Check for new stubs
  run: |
    if grep -r "unimplemented!\|todo!()" src/ --include="*.rs"; then
      echo "Error: New stubs detected. Please complete implementation or use feature flags."
      exit 1
    fi
```

---

## Phase 6: Immediate Action Items

### Week 1: Cleanup

- [ ] Fix compilation error in `package.rs`
- [ ] Remove `#[allow(dead_code)]` from `lib.rs` and `main.rs`
- [ ] Either implement or remove dead code

### Week 2: Feature Flags

- [ ] Simplify feature flags as outlined above
- [ ] Update documentation
- [ ] Update CI matrices

### Week 3: Documentation

- [ ] Create FEATURE_STATUS.md tracking what's implemented
- [ ] Update README with accurate feature list
- [ ] Document the roadmap

### Month 2: Workspace (Optional)

- [ ] Evaluate if codebase size warrants workspace split
- [ ] Plan crate boundaries
- [ ] Migrate incrementally

---

## Success Metrics

Track these to measure improvement:

| Metric | Current | Target |
|--------|---------|--------|
| Feature flags | 20+ | 8-10 |
| Stub modules | 5+ | 0 |
| `#[allow(dead_code)]` | 2 files | 0 files |
| Compile time (clean) | ? | -30% |
| Test time | ? | -20% |
| Documentation accuracy | 80% | 95% |

---

## Communication

### For Users

Be explicit in README:

```markdown
## Current Capabilities

✅ **Production Ready:**
- SSH execution via russh
- Local execution
- Docker containers
- AWS EC2/S3 modules
- Kubernetes pods
- 40+ built-in modules

⚠️ **Experimental:**
- Terraform-like provisioning (AWS only)
- REST API server

❌ **Planned (Not Yet Implemented):**
- Azure cloud modules
- GCP cloud modules
- Database modules
- Windows WinRM support
```

### For Contributors

Add CONTRIBUTING.md section:

```markdown
## Adding New Features

1. Discuss in issue first
2. Create feature branch
3. Complete implementation (no stubs)
4. Add tests
5. Add documentation
6. Request review

**No stub implementations will be merged.**
```

---

## Conclusion

Scope management is about **focus**. By removing stubs, simplifying feature flags, and being honest about what's implemented, you'll:

1. Reduce maintenance burden
2. Improve compile times
3. Set clearer expectations
4. Make the project more approachable

The goal isn't to abandon these features forever—it's to ship a solid core first, then add features properly when they're ready.
