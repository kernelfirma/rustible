---
summary: Reference for the ipoib module that configures IP over InfiniBand network interfaces.
read_when: You need to configure IPoIB interface mode, MTU, or IP addressing from playbooks.
---

# ipoib - IPoIB Interface Configuration

## Synopsis

Configure IPoIB (IP over InfiniBand) network interfaces. Supports setting the transport mode (connected or datagram), MTU, and IP address. Manages interface state via `ip` commands and IPoIB mode via sysfs.

## Classification

**Default** - HPC module. Requires `hpc` and `ofed` feature flags.

## Parameters

| Parameter | Required | Default | Type | Description |
|-----------|----------|---------|------|-------------|
| interface | yes | - | string | Interface name (e.g., `ib0`, `ib1`) |
| mode | no | "datagram" | string | IPoIB transport mode: `connected` or `datagram` |
| mtu | no | mode-dependent | integer | Maximum Transmission Unit. Default is 65520 for connected mode, 2044 for datagram mode. |
| ip_address | no | null | string | IP address with CIDR prefix (e.g., `192.168.1.10/24`) |
| state | no | "present" | string | Desired state: `present` (configure interface) or `absent` (deconfigure interface) |

## Return Values

| Key | Type | Description |
|-----|------|-------------|
| changed | boolean | Whether changes were made |
| msg | string | Status message |
| data.interface | string | Interface name |
| data.mode | string | IPoIB mode applied |
| data.mtu | integer | MTU value applied |
| data.ip_address | string | IP address assigned |
| data.state | string | Current interface state (for absent action) |

## Examples

```yaml
- name: Configure ib0 in datagram mode with IP
  ipoib:
    interface: ib0
    mode: datagram
    ip_address: "192.168.1.10/24"
    state: present

- name: Configure ib0 in connected mode with large MTU
  ipoib:
    interface: ib0
    mode: connected
    mtu: 65520
    ip_address: "10.0.0.1/16"
    state: present

- name: Configure ib1 with default datagram settings
  ipoib:
    interface: ib1
    ip_address: "192.168.2.10/24"

- name: Deconfigure an IPoIB interface
  ipoib:
    interface: ib0
    state: absent
```

## Notes

- Requires building with `--features hpc,ofed` (or `full-hpc`).
- The interface must already exist on the system (i.e., InfiniBand drivers must be loaded). The module does not create virtual interfaces.
- When `state: present`, the module brings the interface up, sets the IPoIB mode via `/sys/class/net/{interface}/mode`, sets the MTU, and optionally assigns an IP address (flushing existing addresses first).
- When `state: absent`, the module flushes IP addresses and brings the interface down.
- Configuration is idempotent: the module checks current mode, MTU, and IP address before making changes.
- Default MTU depends on the mode: 65520 for `connected`, 2044 for `datagram`.
- The `connected` mode offers higher throughput but requires switches that support it. The `datagram` mode is universally compatible.
- Supports check mode for all operations.
