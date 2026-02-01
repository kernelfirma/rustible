---
summary: Benchmark methodology and reproducible performance comparisons.
read_when: You need to validate or reproduce the performance claims in README.
---

# Benchmarks

This document describes how Rustible performance comparisons are produced and how to
re-run them locally or in CI.

## Suites

Rustible includes two benchmark suites:

- **Simulated suite**: Fast, deterministic scenarios used for quick checks.
- **Comparison suite**: Real playbook runs against Rustible and Ansible to validate
  README claims (simple playbook, file copy, template rendering).

## Running the comparison suite

Use the CLI benchmark command to run the playbook comparisons:

```bash
rustible bench --suite comparison --host-count 10 --iterations 3
```

By default, a local inventory is generated with `ansible_connection=local`. If you
want to run against a real inventory, pass `--inventory`:

```bash
rustible bench --suite comparison --inventory path/to/inventory.ini
```

Results are written to `benchmarks/results/summary.json` by default.

## Regression budgets

Use the budgets file to enforce regression limits:

```bash
rustible bench --suite comparison --budgets benchmarks/perf_budgets.toml --baseline benchmarks/results/summary.json
```

Budgets are expressed as maximum regression percent versus a baseline, optional
absolute time limits, and minimum speedup thresholds.

## Playbook sources

Playbooks used for comparisons live in:

- `benchmarks/comparison/playbooks/rustible`
- `benchmarks/comparison/playbooks/ansible`

These playbooks are written to avoid host collisions by including
`{{ inventory_hostname }}` in temporary paths.
