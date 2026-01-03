---
summary: Documentation home page with quick links to guides, reference, architecture, and development resources.
read_when: You're entering the documentation and need to find the right resource for your task.
---

# Rustible Documentation

## Quick Links

| Topic | Link |
|-------|------|
| Getting Started | [guides/quick-start.md](guides/quick-start.md) |
| CLI Reference | [guides/cli-reference.md](guides/cli-reference.md) |
| Module Reference | [reference/modules/](reference/modules/) |
| Architecture | [architecture/ARCHITECTURE.md](architecture/ARCHITECTURE.md) |

## Structure

```
docs/
├── guides/        # User tutorials
├── reference/     # API and module docs
├── architecture/  # Design decisions
└── development/   # Contributor docs
```

## Guides

- [Introduction](guides/01-introduction.md)
- [Playbooks](guides/02-playbooks.md)
- [Inventory](guides/03-inventory.md)
- [Variables](guides/04-variables.md)
- [Modules](guides/05-modules.md)
- [Roles](guides/06-roles.md)
- [Execution Strategies](guides/07-execution-strategies.md)
- [Security & Vault](guides/08-security.md)
- [Templating](guides/09-templating.md)
- [Troubleshooting](guides/troubleshooting.md)
- [Best Practices](guides/best-practices.md)

## Reference

- [Variables](reference/variables.md)
- [Inventory](reference/inventory.md)
- [Modules](reference/modules.md)
- [Callbacks](reference/callbacks.md)

## Performance

Rustible is 5-11x faster than Ansible:

| Metric | Improvement |
|--------|-------------|
| Connection pooling | 11x |
| Overall execution | 5.3x |
| Memory usage | 3.7x less |
