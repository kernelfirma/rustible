# Feature: Achieve 95% Module Parity with Ansible Core Modules

## Problem Statement
Rustible currently has 94 built-in modules compared to Ansible's 3000+ Python modules. While Python fallback exists for unimplemented modules, native Rust modules provide better performance and type safety. Achieving 95% parity with Ansible's core modules is critical for migration viability.

## Current State
- 94 built-in Rust modules
- Python fallback for unimplemented modules
- ~90% core module parity
- Some cloud modules incomplete (Azure, GCP stubs)
- Database modules disabled

## Proposed Solution

### Phase 1: Complete Core Module Parity (v0.2.x)
1. **Priority 1: High-frequency modules**
   - `raw` - Execute raw commands
   - `script` - Run local scripts on remote hosts
   - `meta` - Playbook flow control
   - `fail` - Fail with custom message
   - `add_host` - Dynamically add hosts to inventory
   - `group_by` - Create groups based on facts
   - `assemble` - Assemble configuration from fragments
   - `replace` - Replace strings in files
   - ` tempfile` - Create temporary files
   - `wait_for` - Wait for conditions (already exists, verify complete)

2. **Priority 2: File operations**
   - `fetch` - Fetch files from remote hosts
   - `synchronize` - Rsync-like sync (exists, verify)
   - `unarchive` - Extract archives (exists, verify)
   - `archive` - Create archives (exists, verify)
   - `find` - Find files matching patterns
   - `fileglob` - Find files matching glob patterns

3. **Priority 3: System operations**
   - `mount` - Mount filesystems (exists, verify)
   - `filesystem` - Create filesystems
   - `lvol` - Manage LVM volumes
   - `lvg` - Manage LVM volume groups
   - `pvcreate` - Initialize physical volumes
   - `blockinfile` - Insert/remove blocks in files (exists, verify)

### Phase 2: Cloud Module Enhancement (v0.2.x)
1. **AWS modules (complete coverage)**
   - `aws_ec2_instance` - Manage EC2 instances
   - `aws_s3_bucket` - Manage S3 buckets (exists, verify)
   - `aws_iam_role` - Manage IAM roles
   - `aws_iam_policy` - Manage IAM policies
   - `aws_security_group` - Manage security groups (exists, verify)
   - `aws_ebs_volume` - Manage EBS volumes
   - `aws_rds_instance` - Manage RDS instances (exists, verify)
   - `aws_elb` - Manage Elastic Load Balancers
   - `aws_lambda` - Manage Lambda functions

2. **Azure modules (remove stub status)**
   - `azure_rm_virtualmachine` - Manage VMs
   - `azure_rm_resourcegroup` - Manage resource groups
   - `azure_rm_storageaccount` - Manage storage accounts
   - `azure_rm_networkinterface` - Manage network interfaces
   - `azure_rm_subnet` - Manage subnets

3. **GCP modules (remove stub status)**
   - `gcp_compute_instance` - Manage compute instances
   - `gcp_compute_disk` - Manage disks
   - `gcp_compute_network` - Manage networks
   - `gcp_compute_firewall` - Manage firewall rules

### Phase 3: Database Modules (v0.2.x)
1. **PostgreSQL modules**
   ```rust
   // src/modules/postgres/db.rs
   pub struct PostgresDbModule;
   
   impl Module for PostgresDbModule {
       fn name(&self) -> &'static str { "postgresql_db" }
       
       async fn execute(&self, params: &ModuleParams, ctx: &ModuleContext)
           -> ModuleResult<ModuleOutput> {
           let name = params.get_required("name")?;
           let state = params.get("state").unwrap_or("present");
           let conn_str = params.get_required("conn_str")?;
           
           let mut conn = sqlx::PgConnection::connect(&conn_str).await?;
           
           match state {
               "present" => {
                   sqlx::query("CREATE DATABASE ?")
                       .bind(name)
                       .execute(&mut conn)
                       .await?;
               }
               "absent" => {
                   sqlx::query("DROP DATABASE ?")
                       .bind(name)
                       .execute(&mut conn)
                       .await?;
               }
               _ => return Err(Error::InvalidState),
           }
           
           Ok(ModuleOutput::default().changed(true))
       }
   }
   ```

2. **MySQL modules**
   - `mysql_db` - Manage databases
   - `mysql_user` - Manage users
   - `mysql_replication` - Manage replication

3. **Other databases**
   - `redis` - Redis key management
   - `mongodb` - MongoDB operations
   - `sqlite` - SQLite database management

### Phase 4: Network Device Modules (v0.2.x)
1. **Enhanced network modules**
   - `ios_config` - Cisco IOS configuration
   - `eos_config` - Arista EOS configuration
   - `junos_config` - Juniper Junos configuration
   - `nxos_config` - Cisco NX-OS configuration
   - `vyos_config` - VyOS configuration

2. **Network automation features**
   - Config backup and restore
   - Config diff generation
   - Rollback support

### Phase 5: Windows Modules (v0.3.x)
1. **WinRM backend improvements**
   - Complete WinRM implementation
   - Performance optimization
   - Authentication improvements

2. **Windows modules**
   - `win_copy` - Copy files to Windows
   - `win_feature` - Manage Windows features
   - `win_service` - Manage Windows services
   - `win_package` - Install/Remove software
   - `win_user` - Manage Windows users
   - `win_group` - Manage Windows groups
   - `win_regedit` - Registry operations
   - `win_iis_webapplication` - IIS management

## Expected Outcomes
- 95% parity with Ansible core modules
- Complete coverage of high-frequency modules
- Full AWS module coverage
- Complete Azure/GCP modules
- Working database modules
- Enhanced Windows support

## Success Criteria
- [ ] 150+ native Rust modules implemented
- [ ] 95% of top 100 Ansible modules covered
- [ ] All high-frequency modules implemented
- [ ] Complete AWS module coverage
- [ ] Azure modules out of stub status
- [ ] GCP modules out of stub status
- [ ] Database modules working
- [ ] Windows modules functional
- [ ] Comprehensive test coverage (80%+)
- [ ] Module documentation complete

## Implementation Details

### Module Template
```rust
// src/modules/template_module.rs
use crate::modules::{Module, ModuleContext, ModuleOutput, ModuleResult};
use serde_json::Value;

pub struct TemplateModule;

#[async_trait]
impl Module for TemplateModule {
    fn name(&self) -> &'static str {
        "template_module"
    }
    
    fn description(&self) -> &'static str {
        "Brief description of what this module does"
    }
    
    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }
    
    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
    }
    
    async fn execute(
        &self,
        params: &ModuleParams,
        ctx: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Extract parameters
        let param1 = params.get_required("param1")?;
        let param2 = params.get("param2").unwrap_or("default");
        
        // Execute module logic
        let result = self.do_work(param1, param2, ctx).await?;
        
        // Return output
        Ok(ModuleOutput {
            changed: result.changed,
            failed: false,
            msg: result.message,
            ansible_facts: result.facts,
            ..Default::default()
        })
    }
    
    async fn check(
        &self,
        params: &ModuleParams,
        ctx: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Implement check mode (dry run)
        Ok(ModuleOutput {
            changed: self.estimate_change(params, ctx)?,
            ..Default::default()
        })
    }
}
```

### Module Testing
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestContext;
    
    #[tokio::test]
    async fn test_module_execute() {
        let module = TemplateModule;
        let ctx = TestContext::new();
        
        let params = ModuleParams::from_json(json!({
            "param1": "value1",
            "param2": "value2"
        }));
        
        let result = module.execute(&params, &ctx).await.unwrap();
        assert!(result.changed);
    }
    
    #[tokio::test]
    async fn test_module_check_mode() {
        let module = TemplateModule;
        let ctx = TestContext::new();
        
        let params = ModuleParams::from_json(json!({
            "param1": "value1"
        }));
        
        let result = module.check(&params, &ctx).await.unwrap();
        // Verify check mode behavior
    }
}
```

## Related Issues
- #012: Module Schema Validation
- #013: Module Documentation
- #014: Module Testing

## Additional Notes
This is a **P0 (Critical)** feature as module parity is essential for Ansible migration. Should be prioritized for v0.2.x release.

**Target: 95% parity with Ansible's 100 most-used modules by v0.2.x**
