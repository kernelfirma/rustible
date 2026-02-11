---
summary: Reference for the win_package module that manages Windows packages via Chocolatey, MSI, or Winget.
read_when: You need to install, upgrade, or remove software packages on Windows from playbooks.
---

# win_package - Manage Windows Packages

## Synopsis

Installs, upgrades, or removes Windows packages using multiple providers: Chocolatey,
MSI (msiexec), and Winget. The provider can be selected explicitly or auto-detected
from the package name. Package operations are host-exclusive to prevent conflicts.

## Classification

**RemoteCommand** - Windows module (experimental). Requires `winrm` feature flag.

## Parameters

| Parameter         | Required | Default    | Type   | Description                                                           |
|-------------------|----------|------------|--------|-----------------------------------------------------------------------|
| name              | yes      | -          | string | Package name or path to installer (e.g. `git`, `C:\app.msi`).        |
| state             | no       | `present`  | string | Desired state: `present`, `absent`, `latest`.                         |
| provider          | no       | `auto`     | string | Package provider: `chocolatey`, `msi`, `winget`, `auto`.             |
| version           | no       | -          | string | Specific version to install.                                          |
| source            | no       | -          | string | Package source or repository URL.                                     |
| install_args      | no       | -          | string | Additional arguments passed to the installer.                         |
| uninstall_args    | no       | -          | string | Additional arguments passed to the uninstaller.                       |
| product_id        | no       | -          | string | MSI product GUID, required for MSI uninstallation.                    |
| creates           | no       | -          | string | Path that, if it exists, indicates the package is already installed.  |
| allow_prerelease  | no       | `false`    | bool   | Allow prerelease packages (Chocolatey only).                          |
| ignore_checksums  | no       | `false`    | bool   | Ignore package checksums (Chocolatey only).                           |
| force             | no       | `false`    | bool   | Force reinstall even when already present.                            |

## Return Values

| Key             | Type   | Description                                           |
|-----------------|--------|-------------------------------------------------------|
| version         | string | Installed package version (when unchanged).            |
| old_version     | string | Previous version before upgrade (Chocolatey `latest`). |
| new_version     | string | New version after upgrade (Chocolatey `latest`).       |
| reboot_required | bool   | Whether a reboot is needed (MSI exit code 3010).       |

## Examples

```yaml
- name: Install Git via Chocolatey
  win_package:
    name: git
    provider: chocolatey
    state: present

- name: Install a specific version of Node.js
  win_package:
    name: nodejs
    version: "18.17.1"
    provider: chocolatey

- name: Install MSI package with custom args
  win_package:
    name: C:\installers\myapp.msi
    provider: msi
    install_args: "/qn ALLUSERS=1"
    product_id: "{12345678-ABCD-EFGH-IJKL-123456789012}"

- name: Install VS Code via Winget
  win_package:
    name: Microsoft.VisualStudioCode
    provider: winget
    state: present
```

## Notes

- Requires building with `--features winrm`.
- When `provider: auto`, files ending in `.msi` use the MSI provider; all others default to Chocolatey.
- Chocolatey is automatically installed on the target if not already present.
- MSI uninstallation requires the `product_id` parameter.
- The `creates` parameter provides a fast path to skip installation if a sentinel path exists.
