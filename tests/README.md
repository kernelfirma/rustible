# Test Suite

## Running Tests

```bash
cargo test                    # All tests
cargo test --lib              # Unit tests only
cargo test --tests            # Integration tests only
cargo test -- --nocapture     # With output
cargo test test_name          # Specific test
cargo bench                   # Benchmarks
```

## Structure

```
tests/
├── common/           # Shared utilities and mocks
├── fixtures/         # Test data (playbooks, inventories)
├── executor_tests.rs # Execution engine
├── module_tests.rs   # Module system
├── connection_tests.rs
├── inventory_tests.rs
├── template_tests.rs
├── vault_tests.rs
└── integration_tests.rs
```

## Test Utilities

```rust
use common::*;

// Mock connection
let mock = MockConnection::new("host");
mock.set_command_result("cmd", CommandResult::success("out", ""));

// Build test data
let playbook = PlaybookBuilder::new("Test")
    .add_play(PlayBuilder::new("Setup", "hosts").build())
    .build();

// Load fixtures
let playbook = load_playbook_fixture("minimal_playbook")?;
```

## Fixtures

| Directory | Contents |
|-----------|----------|
| `fixtures/playbooks/` | Sample playbooks |
| `fixtures/inventories/` | Sample inventories |
| `fixtures/roles/` | Sample roles |
| `fixtures/templates/` | Jinja2 templates |

## Troubleshooting

```bash
# Debug output
RUST_LOG=debug cargo test -- --nocapture

# Single-threaded (for race conditions)
cargo test -- --test-threads=1

# Run ignored tests
cargo test -- --ignored
```
