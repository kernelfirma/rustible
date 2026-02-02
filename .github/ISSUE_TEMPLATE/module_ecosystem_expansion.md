#183 [CRITICAL] Strategic Module Ecosystem Expansion

## Problem Statement
Rustible has only ~48 built-in modules compared to Ansible's 3000+ modules. This 98% deficit forces users to rely on Python fallback, negating performance benefits and creating Python dependency requirements.

## Current Module Count
| Category | Rustible | Ansible | Gap |
|----------|----------|---------|-----|
| **Core modules** | ~48 | ~300 | 84% |
| **Cloud modules** | ~10 (AWS partial) | ~400 | 97% |
| **Network modules** | 4 | ~150 | 97% |
| **Windows modules** | 5 stubs | ~60 | 92% |
| **Database modules** | 0 (disabled) | ~25 | 100% |
| **Monitoring modules** | 0 | ~40 | 100% |
| **Source control** | 1 (git) | ~15 | 93% |
| **Web/API modules** | 2 | ~50 | 96% |
| **Total** | **~48** | **~3000** | **98%** |

## Module Priority Matrix

### Critical Priority (Blocking Basic Usage)
| Module | Use Case | Effort |
|--------|----------|--------|
| `mount` | Filesystem mounting | Low |
| `filesystem` | FS creation (ext4, xfs) | Medium |
| `lvg` / `lvol` | LVM management | Medium |
| `parted` | Partition management | Medium |
| `zfs` | ZFS management | High |
| `archive` / `unarchive` | Complete tar/zip support | Medium |
| `fetch` | Download from remote hosts | Low |
| `get_url` | HTTP downloads | Low |
| `uri` | HTTP API calls (enhance) | Low |

### High Priority (Common Operations)
| Module | Use Case | Effort |
|--------|----------|--------|
| `mysql_db` / `mysql_user` | MySQL management | Medium |
| `postgresql_db` / `postgresql_user` | PostgreSQL management | Medium |
| `mongodb_*` | MongoDB operations | High |
| `redis` | Redis configuration | Medium |
| `rabbitmq_*` | RabbitMQ management | Medium |
| `kafka_*` | Kafka operations | High |
| `elasticsearch` | ES operations | High |
| `docker_*` (complete) | Container management | Medium |
| `podman_*` | Podman containers | Medium |
| `k8s` modules (expand) | Kubernetes operations | High |

### Medium Priority (Specialized Use Cases)
| Module | Use Case | Effort |
|--------|----------|--------|
| `nmcli` | NetworkManager | Medium |
| `iptables` / `nftables` | Firewall rules | Medium |
| `selinux` | SELinux management | Medium |
| `apparmor` | AppArmor profiles | Medium |
| `pam_limits` | Resource limits | Low |
| `sysctl` (enhance) | Kernel parameters | Low |
| `modprobe` | Kernel modules | Low |
| `kernel_blacklist` | Blacklist modules | Low |

### Network Device Modules
| Module | Use Case | Effort |
|--------|----------|--------|
| `ios_*` (complete) | Cisco IOS | High |
| `eos_*` | Arista EOS | High |
| `junos_*` | Juniper JunOS | High |
| `nxos_*` | Cisco NX-OS | High |
| `vyos_*` | VyOS/Vyatta | High |
| `f5_*` | F5 BIG-IP | Very High |
| `fortios_*` | Fortinet | High |

## Proposed Implementation Strategy

### Phase 1: Module Development Framework
```rust
// src/modules/framework.rs
pub struct ModuleGenerator;

impl ModuleGenerator {
    /// Generate module stub from specification
    pub fn generate_module_stub(spec: ModuleSpec) -> TokenStream {
        // Generate boilerplate:
        // - Module struct
        // - Parameter validation
    - Documentation
        // - Error handling
        // - Idempotency checks
    }
    
    /// Generate tests from specification
    pub fn generate_tests(spec: ModuleSpec) -> TokenStream {
        // Generate unit tests
        // Generate integration tests
    }
}
```

- [ ] Module generator tool
- [ ] Standardized module template
- [ ] Testing framework for modules
- [ ] Documentation generator
- [ ] Module validation tool

### Phase 2: Community Module Index
```yaml
# modules/index.yml
modules:
  - name: mount
    category: system
    priority: critical
    status: not_started
    owner: core_team
    
  - name: mysql_db
    category: database
    priority: high
    status: not_started
    owner: community
    
  - name: docker_container
    category: container
    priority: high
    status: partial
    owner: core_team
```

- [ ] Public module roadmap
- [ ] Community contribution guidelines
- [ ] Module request tracking
- [ ] Bounty program for critical modules

### Phase 3: High-Priority Module Implementation

#### Database Modules
```rust
// src/modules/database/mysql.rs
pub struct MysqlDb;
pub struct MysqlUser;
pub struct MysqlReplication;

impl Module for MysqlDb {
    fn name(&self) -> &'static str { "mysql_db" }
    
    fn execute(&self, params: &ModuleParams, ctx: &ModuleContext) -> ModuleResult {
        // Create, drop, modify databases
        // Handle collation and charset
        // State check for idempotency
    }
}
```

- [ ] `mysql_db` / `mysql_user` / `mysql_replication`
- [ ] `postgresql_db` / `postgresql_user` / `postgresql_privs`
- [ ] `mongodb_user` / `mongodb_replicaset`
- [ ] `redis` configuration
- [ ] `elasticsearch_index`

#### Filesystem Modules
- [ ] `mount` (complete with all FS types)
- [ ] `filesystem` (mkfs operations)
- [ ] `lvg` / `lvol` (LVM)
- [ ] `parted` (partitioning)
- [ ] `zfs` (if feasible)

#### Web/API Modules
- [ ] `get_url` (downloads with checksums)
- [ ] `uri` (enhanced HTTP methods)
- [ ] `graphql` queries
- [ ] `rest_api` generic module

#### Container Modules
- [ ] `docker_container` (complete)
- [ ] `docker_image` (pull, build)
- [ ] `docker_network`
- [ ] `docker_volume`
- [ ] `docker_compose`
- [ ] `podman_container`
- [ ] `kubernetes` modules (expand)

### Phase 4: Network Device Modules
```rust
// src/modules/network/ios.rs
pub struct IosCommand;
pub struct IosConfig;
pub struct IosVlan;
pub struct IosInterface;
```

- [ ] Complete Cisco IOS modules
- [ ] Arista EOS modules
- [ ] Juniper JunOS modules
- [ ] Common network abstraction layer

### Phase 5: Python Module Bridge Enhancement
Since full module parity will take years, enhance Python fallback:

```rust
// src/modules/python_bridge.rs
pub struct PythonModuleBridge {
    module_name: String,
    ansible_module: String,
    optimization: BridgeOptimization,
}

impl PythonModuleBridge {
    /// Cache Python module compilation
    pub fn warm_cache(&self) {
        // Pre-compile Python modules
        // Cache AnsiballZ scripts
    }
    
    /// Batch execution for multiple hosts
    pub async fn execute_batch(&self, hosts: &[Host], params: &ModuleParams) -> Vec<ModuleResult> {
        // Execute Python module on multiple hosts efficiently
    }
}
```

- [ ] Python module caching
- [ ] Batch execution optimization
- [ ] Reduced Python overhead
- [ ] Better error translation

## Module Quality Standards

### Idempotency Requirements
```rust
pub fn check_idempotency(module: &dyn Module, test_cases: &[TestCase]) {
    for case in test_cases {
        // First run - should report changed
        let result1 = module.execute(&case.params, &case.context);
        assert!(result1.changed);
        
        // Second run - should NOT report changed
        let result2 = module.execute(&case.params, &case.context);
        assert!(!result2.changed);
    }
}
```

### Testing Requirements
- [ ] Unit tests for all parameters
- [ ] Integration tests with real systems
- [ ] Idempotency tests
- [ ] Error handling tests
- [ ] Check mode support tests
- [ ] Diff mode support tests

## Progress Tracking

### Target: 200 Modules by Beta
| Quarter | Target | Focus |
|---------|--------|-------|
| Q1 | 75 modules | Core + Database |
| Q2 | 125 modules | Container + Web |
| Q3 | 175 modules | Network + Cloud |
| Q4 | 200 modules | Specialized |

### Target: 500 Modules by 1.0
| Category | Count |
|----------|-------|
| Core/System | 100 |
| Database | 50 |
| Web/API | 50 |
| Cloud | 100 |
| Container | 50 |
| Network Devices | 100 |
| Monitoring | 25 |
| Other | 25 |
| **Total** | **500** |

## Acceptance Criteria
- [ ] 200 modules by Beta release
- [ ] All critical priority modules complete
- [ ] 80% of modules have integration tests
- [ ] Module documentation complete
- [ ] Python fallback usage <20% for common operations
- [ ] Community contribution process established

## Priority
**CRITICAL** - Module ecosystem is the biggest barrier to adoption

## Related
- Issue #172: Windows support (includes Windows modules)
- Issue #181: Cloud provider modules
- Issue #163: Python module local execution (fallback optimization)

## Labels
`critical`, `modules`, `ecosystem`, `ansible-parity`, `community`
