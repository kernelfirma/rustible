---
summary: Reference for the ipmi_boot module that sets server boot device via IPMI using ipmitool.
read_when: You need to set the boot device (PXE, disk, CD-ROM, BIOS) on servers via IPMI from playbooks.
---

# ipmi_boot - Set Server Boot Device via IPMI

## Synopsis

Sets the server boot device via IPMI using `ipmitool chassis bootdev`. Supports
PXE, disk, CD-ROM, and BIOS boot targets with optional persistent boot override.
Includes idempotency checks against the current boot device configuration.

## Classification

**Default** - HPC module. Requires `hpc` feature flag.

## Parameters

| Parameter  | Required | Default   | Type    | Description                                                   |
|------------|----------|-----------|---------|---------------------------------------------------------------|
| host       | yes      | -         | string  | BMC/IPMI hostname or IP address.                              |
| device     | yes      | -         | string  | Boot device: `pxe`, `disk`, `cdrom`, or `bios`.              |
| user       | no       | `admin`   | string  | IPMI username.                                                |
| password   | no       | `""`      | string  | IPMI password.                                                |
| interface  | no       | `lanplus` | string  | IPMI interface type (e.g. `lanplus`, `lan`).                  |
| persistent | no       | `false`   | boolean | Whether the boot device setting should persist across reboots.|

## Return Values

| Key     | Type    | Description                                                      |
|---------|---------|------------------------------------------------------------------|
| changed | boolean | Whether the boot device was changed.                             |
| msg     | string  | Status message.                                                  |
| data    | object  | Contains `host`, `device`, `persistent`, and `previous_device` fields. |

## Examples

```yaml
- name: Set PXE boot for provisioning
  ipmi_boot:
    host: 10.0.0.101
    device: pxe
    user: admin
    password: "{{ ipmi_password }}"

- name: Set disk boot persistently
  ipmi_boot:
    host: 10.0.0.101
    device: disk
    persistent: true
    user: admin
    password: "{{ ipmi_password }}"

- name: Boot into BIOS setup
  ipmi_boot:
    host: 10.0.0.101
    device: bios
    user: admin
    password: "{{ ipmi_password }}"

- name: Set CD-ROM boot
  ipmi_boot:
    host: 10.0.0.101
    device: cdrom
    user: admin
    password: "{{ ipmi_password }}"
```

## Notes

- Requires building with `--features hpc`.
- Requires `ipmitool` to be installed on the managed host.
- The module is idempotent: if the boot device is already set to the desired value, no change is made.
- The `persistent` option appends `options=persistent` to the `ipmitool chassis bootdev` command, making the boot device setting survive reboots.
- Boot device detection parses `ipmitool chassis bootparam get 5` output.
- Passwords containing single quotes are automatically escaped.
