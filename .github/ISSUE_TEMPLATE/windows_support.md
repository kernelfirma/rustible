#172 [CRITICAL] Production-Ready Windows Support (WinRM + PowerShell)

## Problem Statement
Rustible currently lacks production-ready Windows support. The `winrm` feature is marked experimental and the 5 Windows modules (`win_copy`, `win_service`, etc.) are stubs. This blocks adoption in mixed environments where Windows servers are common (typically 30-50% of enterprise infrastructure).

## Current State
- WinRM connection: Experimental stub (`src/connection/winrm.rs` - feature gated)
- Windows modules: 5 stub implementations only
- PowerShell execution: Not implemented
- Windows facts gathering: Not implemented
- Active Directory modules: Not implemented

## Comparison to Ansible
| Feature | Rustible | Ansible 2.15+ |
|---------|----------|---------------|
| WinRM connection | ❌ Experimental stub | ✅ Full WinRM + NTLM/Kerberos |
| Windows modules | ❌ 5 stubs | ✅ 50+ production modules |
| PowerShell execution | ❌ Not implemented | ✅ Native PowerShell + module support |
| Windows facts | ❌ Not implemented | ✅ Comprehensive Windows fact collection |
| Active Directory | ❌ Not implemented | ✅ Full AD module suite |
| Chocolatey/WinGet | ❌ Not implemented | ✅ Package manager modules |

## Proposed Implementation

### Phase 1: Core WinRM Connection
- [ ] Complete WinRM connection implementation (remove experimental flag)
- [ ] NTLM authentication support
- [ ] Kerberos authentication support
- [ ] Certificate-based authentication
- [ ] Connection pooling for WinRM
- [ ] HTTPS/SSL support for WinRM

### Phase 2: PowerShell Execution
- [ ] PowerShell command execution module
- [ ] PowerShell script execution with argument passing
- [ ] PowerShell module installation support
- [ ] Error parsing and structured output
- [ ] PowerShell remoting over WinRM

### Phase 3: Core Windows Modules
- [ ] `win_copy` - File copy with ACL preservation
- [ ] `win_file` - File/directory management
- [ ] `win_service` - Windows service management
- [ ] `win_user` / `win_group` - Local user/group management
- [ ] `win_package` - MSI/EXE package installation
- [ ] `win_chocolatey` - Chocolatey package manager
- [ ] `win_winget` - Windows Package Manager (WinGet)
- [ ] `win_firewall` - Windows Firewall rules
- [ ] `win_reg` - Windows registry management
- [ ] `win_iis_website` / `win_iis_webapp` - IIS management

### Phase 4: Windows Facts
- [ ] Windows-specific fact gathering module
- [ ] Registry-based facts
- [ ] WMI-based system information
- [ ] Domain/AD membership detection
- [ ] Windows feature detection

## Acceptance Criteria
- [ ] Can execute playbooks against Windows Server 2019/2022
- [ ] All core modules have idempotency guarantees
- [ ] NTLM and Kerberos authentication work in enterprise environments
- [ ] Performance within 2x of equivalent Ansible modules
- [ ] Integration tests against real Windows hosts pass

## Priority
**CRITICAL** - Blocks enterprise adoption in mixed environments

## Related
- Feature flag: `winrm` in `Cargo.toml`
- Existing stubs: `src/modules/windows/`
- Connection: `src/connection/winrm.rs`

## Labels
`critical`, `platform-support`, `windows`, `enterprise-readiness`
