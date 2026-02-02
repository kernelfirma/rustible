#176 [HIGH] Distributed Execution for Large-Scale Infrastructures

## Problem Statement
Rustible executes all tasks from a single control node, limiting scalability to what one machine can handle. This creates bottlenecks for large inventories (1000+ hosts) and prevents high-availability deployment patterns. Unlike Ansible Tower/AWX, Rustible has no distributed execution capability.

## Current State
- **Max tested scale**: ~100 hosts with real SSH
- **Architecture**: Single control node, all connections originate from one machine
- **Forks**: Configurable but limited by single-node resources
- **No failover**: Control node failure stops entire deployment

## Scalability Limitations
| Metric | Rustible | Ansible + Tower | Impact |
|--------|----------|-----------------|--------|
| Max hosts (tested) | ~100 | 10,000+ | Untested at scale |
| Control node failover | ❌ None | ✅ HA clustering | Single point of failure |
| Geographic distribution | ❌ Centralized | ✅ Mesh execution | Network latency issues |
| Resource bottleneck | CPU/Network | Distributed | Single node limits |

## Comparison to Alternatives
| Feature | Rustible | Ansible Tower | Terraform Cloud |
|---------|----------|---------------|-----------------|
| Single-node execution | ✅ | ✅ | ✅ |
| Multi-node execution | ❌ | ✅ | ✅ |
| Execution queuing | ❌ | ✅ | ✅ |
| Job scheduling | ❌ | ✅ | ✅ |
| Callback-based execution | ❌ | ✅ (push via callback) | ✅ |
| HA control plane | ❌ | ✅ | ✅ (SaaS) |

## Use Cases Blocked
1. **5000+ host inventories** - Exceeds single-node connection capacity
2. **Multi-region deployments** - High latency from single control node
3. **High-availability requirements** - No failover if control node fails
4. **CI/CD at scale** - Build agents overwhelm single execution node

## Proposed Implementation

### Architecture Options

#### Option A: Agent-Based (Similar to Ansible Tower)
```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Control    │────▶│  Execution  │────▶│   Target    │
│   Node      │     │    Nodes    │     │   Hosts     │
│ (API/Queue) │     │  (Agents)   │     │             │
└─────────────┘     └─────────────┘     └─────────────┘
```

#### Option B: Mesh Topology (Decentralized)
```
┌─────────┐ ──── ┌─────────┐
│ Node A  │◄────►│ Node B  │
└────┬────┘      └────┬────┘
     │                │
     └──────┬─────────┘
            │
       ┌────┴────┐
       │ Shared  │
       │  Queue  │
       └─────────┘
```

### Phase 1: Execution Node Agent
```rust
// src/distributed/agent.rs
pub struct ExecutionAgent {
    node_id: Uuid,
    control_plane_url: Url,
    max_concurrent_jobs: usize,
    heartbeat_interval: Duration,
}

impl ExecutionAgent {
    pub async fn run(&self) -> Result<(), AgentError> {
        // Register with control plane
        // Heartbeat loop
        // Poll for jobs
        // Execute playbooks against assigned hosts
        // Report results
    }
}
```

- [ ] Create agent binary (`rustible-agent`)
- [ ] Agent registration with control plane
- [ ] Heartbeat mechanism
- [ ] Job polling/assignment
- [ ] Result reporting
- [ ] Agent authentication (mTLS or tokens)

### Phase 2: Control Plane
```rust
// src/distributed/control_plane.rs
pub struct ControlPlane {
    job_queue: Arc<dyn JobQueue>,
    agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
    scheduler: Box<dyn JobScheduler>,
}

impl ControlPlane {
    pub async fn submit_job(&self, job: Job) -> Result<JobId, ControlPlaneError> {
        // Queue job
        // Assign to available agent(s)
        // Track progress
    }
}
```

- [ ] Job queue implementation (Redis/RabbitMQ/SQLite)
- [ ] Agent management and health tracking
- [ ] Job scheduler (round-robin, load-based, geographic)
- [ ] REST API for job submission
- [ ] WebSocket for real-time updates

### Phase 3: Host Partitioning
```rust
// Distribute hosts across agents
pub fn partition_hosts(
    hosts: Vec<Host>,
    agents: Vec<Agent>,
    strategy: PartitionStrategy,
) -> Vec<HostPartition> {
    match strategy {
        PartitionStrategy::RoundRobin => /* distribute evenly */,
        PartitionStrategy::Geographic => /* by region/datacenter */,
        PartitionStrategy::LoadBased => /* based on agent capacity */,
    }
}
```

- [ ] Host partitioning strategies
- [ ] Geographic affinity (agents in same region as targets)
- [ ] Dynamic rebalancing
- [ ] Fault tolerance (retry failed partitions on other agents)

### Phase 4: API Server Mode
```bash
# Start control plane
rustible server --bind 0.0.0.0:8080

# Agents connect
rustible agent --server https://rustible.internal:8080 --token $AGENT_TOKEN

# Submit job via API
curl -X POST https://rustible.internal:8080/api/v1/jobs \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "playbook": "deploy.yml",
    "inventory": "production",
    "limit": "webservers"
  }'
```

- [ ] HTTP API for job submission
- [ ] Authentication/authorization
- [ ] Job status endpoints
- [ ] Webhook callbacks
- [ ] Job logs streaming

### Phase 5: HA and Failover
- [ ] Control plane clustering (Raft/consensus)
- [ ] Agent auto-discovery
- [ ] Job checkpointing for agent failover
- [ ] Leader election for control plane
- [ ] Database backend for job persistence

## Configuration
```toml
# rustible.toml - Control plane mode
[server]
enabled = true
bind = "0.0.0.0:8080"
database_url = "postgres://rustible:pass@db/rustible"

[server.queue]
type = "redis"
url = "redis://redis:6379"

[[agents]]
id = "agent-01"
region = "us-east-1"
capacity = 100  # max concurrent hosts

[[agents]]
id = "agent-02"
region = "eu-west-1"
capacity = 100
```

```toml
# Agent configuration
[agent]
server_url = "https://rustible.internal:8080"
token = "${AGENT_TOKEN}"
heartbeat_interval = 30
max_concurrent_jobs = 10
```

## Acceptance Criteria
- [ ] Can execute playbook against 1000+ hosts via 10 agents
- [ ] Agent failure doesn't lose job progress (checkpointing)
- [ ] Geographic affinity reduces latency
- [ ] Control plane HA (no single point of failure)
- [ ] API allows external CI/CD integration
- [ ] Job queue persists across control plane restarts
- [ ] Performance scales linearly with agent count

## Priority
**HIGH** - Required for enterprise-scale deployments (1000+ hosts)

## Related
- Performance benchmarks show untested beyond 100 hosts
- Connection pool limits (50 total) need distributed solution

## Labels
`high`, `distributed`, `scalability`, `enterprise`, `feature-request`
