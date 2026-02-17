---
summary: Reference for the ib_diagnostics module that runs InfiniBand diagnostic tools and parses results.
read_when: You need to run InfiniBand health checks, topology discovery, or error counter queries from playbooks.
---

# ib_diagnostics - InfiniBand Diagnostics

## Synopsis

Run InfiniBand diagnostic tools (ibdiagnet, iblinkinfo, ibstat, ibnetdiscover, ibqueryerrors) and parse results for link health issues and error counters. Diagnostic output is saved to report files for later review.

## Classification

**Default** - HPC module. Requires `hpc` and `ofed` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| check | yes | - | string | Diagnostic type to run: `link_health`, `topology`, `counters`, or `full` |
| port_filter | no | null | string | Filter results by port or HCA (passed as `-C` flag to diagnostic tools) |
| output_dir | no | "/tmp/ib_diagnostics" | string | Directory to store diagnostic report files |

## Diagnostic Check Types

| Check | Tools Executed | Purpose |
|-------|---------------|---------|
| link_health | iblinkinfo, ibstat | Check link states and port health |
| topology | ibnetdiscover, iblinkinfo | Discover fabric topology and link connections |
| counters | ibqueryerrors | Query port error counters |
| full | ibdiagnet, iblinkinfo, ibstat, ibqueryerrors | Run all diagnostic tools |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Always false (read-only diagnostics) |
| msg | string | Summary message with issue count |
| data.check | string | Diagnostic check type that was run |
| data.output_dir | string | Directory where reports were saved |
| data.issues | array | List of detected issues (e.g., link state problems, error counters) |
| data.errors | array | List of tool execution errors (tools that failed to run) |
| data.results | object | Map of tool name to raw stdout output |

## Examples

```yaml
- name: Run link health diagnostics
  ib_diagnostics:
    check: link_health

- name: Run full diagnostics with port filter
  ib_diagnostics:
    check: full
    port_filter: mlx5_0
    output_dir: /var/log/ib_diagnostics

- name: Check error counters
  ib_diagnostics:
    check: counters

- name: Discover fabric topology
  ib_diagnostics:
    check: topology
    output_dir: /tmp/fabric_topology
```

## Notes

- Requires building with `--features hpc,ofed` (or `full-hpc`).
- Requires OFED InfiniBand diagnostic tools to be installed on the target host.
- The module creates the output directory if it does not exist.
- Each diagnostic tool's output is saved to `{output_dir}/{tool_name}.log`.
- Link health issues are detected by scanning iblinkinfo output for `Down` or `Polling` states.
- Error counters are flagged when ibqueryerrors output is non-empty and does not contain `No errors`.
- Tools that fail to execute are reported in the `errors` array but do not cause the module to fail.
- Supports check mode (reports what diagnostics would be run without executing them).
