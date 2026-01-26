# Ansible Compatibility Test Harness

This directory contains the Ansible compatibility test harness for Rustible. It verifies that Rustible behaves consistently with Ansible across key modules and features.

## Structure

```
tests/compat/
├── mod.rs                      # Test harness and behavior matrix
├── README.md                   # This file
└── fixtures/
    ├── playbooks/              # Test playbook YAML files
    │   ├── file_operations.yml
    │   ├── package_operations.yml
    │   ├── template_operations.yml
    │   ├── service_operations.yml
    │   └── user_operations.yml
    └── golden/                 # Expected outputs (for regression testing)
```

## Running Tests

```bash
# Run all compatibility tests
cargo test compat_

# Run specific test
cargo test compat_file_operations

# Run with verbose output
cargo test compat_ -- --nocapture
```

## Test Fixtures

| Fixture | Modules Tested | Description |
|---------|----------------|-------------|
| `file_operations.yml` | file, copy, stat | File/directory management |
| `package_operations.yml` | apt, yum, package | Package installation |
| `template_operations.yml` | template, debug | Jinja2 templating |
| `service_operations.yml` | service, systemd | Service management |
| `user_operations.yml` | user, group | User/group management |

## Module Behavior Matrix

The test harness includes a behavior matrix tracking the top 20 modules by usage:

```rust
use rustible::tests::compat::behavior_matrix;

// Get compatibility percentage
let pct = behavior_matrix::compatibility_percentage();
println!("Compatibility: {:.1}%", pct);

// Check specific module
for module in behavior_matrix::TOP_20_MODULES {
    println!("{}: {:?}", module.name, module.status);
}
```

## Adding New Fixtures

1. Create a playbook in `fixtures/playbooks/`
2. (Optional) Run with Ansible to generate golden output
3. Add a test case in `mod.rs`
4. Update the behavior matrix if adding a new module

## CI Integration

The compatibility tests run as part of the standard test suite:

```yaml
# .github/workflows/ci.yml
- name: Run compatibility tests
  run: cargo test compat_ --release
```

## Related Documentation

- [Ansible Compatibility Matrix](../../docs/compatibility/ansible.md)
- [Existing ansible_compat tests](../ansible_compat/)
- [Module Health Dashboard](../../docs/MODULES_HEALTH.md)
