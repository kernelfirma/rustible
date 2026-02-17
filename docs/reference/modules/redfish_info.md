---
summary: Reference for the redfish_info module that queries system information via Redfish REST API.
read_when: You need to retrieve server inventory, sensor data, thermal readings, or power consumption from playbooks.
---

# redfish_info - Query System Information via Redfish

## Synopsis

Query system information from Redfish-compliant BMC REST API endpoints. Supports querying system inventory, chassis details, thermal readings, power consumption, and storage information. Uses curl for REST API calls.

## Classification

**Default** - HPC module. Requires `hpc` and `redfish` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| host | yes | - | string | BMC hostname or IP address |
| query_type | yes | - | string | Type of information to query: "system", "chassis", "thermal", "power", or "storage" |
| user | no | "admin" | string | Redfish authentication username |
| password | no | "" | string | Redfish authentication password |
| verify_ssl | no | false | boolean | Whether to verify SSL certificates |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Always false (read-only operation) |
| msg | string | Status message |
| data.host | string | The BMC host address |
| data.query_type | string | The query type that was executed |
| data.endpoint | string | The Redfish API endpoint that was queried |
| data.response | object | The full JSON response from the Redfish API |

## Examples

```yaml
- name: Get system inventory information
  redfish_info:
    host: "bmc-compute-001.hpc.local"
    query_type: system
    user: admin
    password: "{{ bmc_password }}"

- name: Query thermal readings
  redfish_info:
    host: "10.0.1.100"
    query_type: thermal
    user: admin
    password: "{{ bmc_password }}"
    verify_ssl: false

- name: Get power consumption data
  redfish_info:
    host: "bmc-compute-001.hpc.local"
    query_type: power
    user: admin
    password: "{{ bmc_password }}"

- name: Query storage information
  redfish_info:
    host: "bmc-compute-001.hpc.local"
    query_type: storage
    user: admin
    password: "{{ bmc_password }}"
```

## Notes

- Requires building with `--features hpc,redfish` or `--features full-hpc`.
- The `curl` command must be available on the target host.
- This is a read-only module; it never reports `changed: true`.
- Query types map to Redfish API endpoints: "system" -> `/redfish/v1/Systems/1`, "chassis" -> `/redfish/v1/Chassis/1`, "thermal" -> `/redfish/v1/Chassis/1/Thermal`, "power" -> `/redfish/v1/Chassis/1/Power`, "storage" -> `/redfish/v1/Systems/1/Storage`.
- The full JSON response from the BMC is returned in the `data.response` field.
- SSL verification is disabled by default for self-signed BMC certificates.
- This module uses `FullyParallel` parallelization, meaning multiple BMC queries can run concurrently.
