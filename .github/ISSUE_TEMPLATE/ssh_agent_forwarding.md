#174 [HIGH] Implement SSH Agent Forwarding

## Problem Statement
Rustible currently supports SSH agent authentication (using local SSH agent) but does not implement **SSH Agent Forwarding** (forwarding the agent to remote hosts). This blocks workflows where:
- Git operations need to authenticate via SSH on remote hosts
- SSH connections need to be made from target hosts to other hosts
- Multi-hop deployments require credential forwarding

## Current State
- SSH agent authentication: ✅ Supported (`russh-keys` integration)
- SSH agent forwarding: ❌ Not implemented
- SSH agent socket handling: ❌ Not implemented

## Comparison to Ansible
| Feature | Rustible | Ansible |
|---------|----------|---------|
| SSH agent auth | ✅ Supported | ✅ Supported |
| Agent forwarding (`-A`) | ❌ Not implemented | ✅ `ssh_args: -o ForwardAgent=yes` |
| Agent socket cleanup | ❌ N/A | ✅ Automatic |

## Use Cases Blocked
1. **Git clone via SSH on remote hosts** - Requires forwarding local SSH agent
2. **Deployment to private Git repositories** - Git operations on targets need auth
3. **Multi-tier deployments** - App server → Database server connections
4. **Bastion host workflows** - Credentials forwarded through jump hosts

## Proposed Implementation

### Core Implementation
```rust
// src/connection/ssh.rs
pub struct SshConnectionConfig {
    // Add agent forwarding option
    pub forward_agent: bool,
    pub agent_socket_path: Option<PathBuf>,
}

impl SshConnection {
    async fn setup_agent_forwarding(&self) -> Result<AgentForwardChannel> {
        if self.config.forward_agent {
            // Set up Unix socket on remote host
            // Forward agent requests to local SSH agent
            // Handle agent protocol
        }
    }
}
```

### Tasks
- [ ] Add `forward_agent: bool` to SSH connection configuration
- [ ] Implement SSH agent protocol handling
- [ ] Create remote Unix socket for agent forwarding
- [ ] Bridge remote agent requests to local agent
- [ ] Add `SSH_AUTH_SOCK` environment variable injection
- [ ] Implement proper socket cleanup on disconnect

### Configuration Options
```yaml
# ansible.cfg equivalent
[ssh]
forward_agent = true

# Or per-host in inventory
webservers:
  hosts:
    web1:
      ansible_host: 192.168.1.10
      ansible_ssh_forward_agent: true
```

### CLI Support
```bash
# Global flag
rustible run playbook.yml -i inventory.yml --forward-agent

# Config file option
rustible config set ssh.forward_agent true
```

### Security Considerations
- [ ] Document security implications of agent forwarding
- [ ] Add warning when enabling agent forwarding to untrusted hosts
- [ ] Support agent forwarding with restrictions (specific keys only)
- [ ] Consider `ForwardAgent` vs `AddKeysToAgent` options

## Acceptance Criteria
- [ ] `ssh-add -l` on remote host shows local agent keys (when enabled)
- [ ] Git clone from private repos works on remote hosts
- [ ] Agent socket properly cleaned up on disconnect
- [ ] Works with jump hosts (multi-hop forwarding)
- [ ] Can be disabled per-host in inventory
- [ ] Security documentation covers risks and best practices

## Priority
**HIGH** - Blocks common deployment workflows; Ansible parity feature

## Related
- Issue #166: Keyboard-interactive SSH auth (related SSH feature)
- Issue #168: russh_auth API update (may affect implementation)

## Labels
`high`, `ssh`, `feature-parity`, `ansible-compatible`
