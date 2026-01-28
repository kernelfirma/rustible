# Feature: Implement State Locking for Team Collaboration

## Problem Statement
When multiple team members or CI/CD pipelines run Rustible concurrently, they can corrupt the state file or create conflicting changes. State locking prevents this by ensuring only one process can modify the state at a time.

## Current State
- No state locking mechanism
- Concurrent modifications can corrupt state
- No coordination between team members
- No protection against CI/CD conflicts

## Proposed Solution

### Phase 1: DynamoDB Locking (v0.3.x)
1. **DynamoDB implementation**
   ```rust
   // src/state/lock/dynamodb.rs
   use aws_sdk_dynamodb::{Client as DynamoDbClient, types::AttributeValue};
   
   pub struct DynamoDBLock {
       table: String,
       key: String,
       client: DynamoDbClient,
       timeout: Duration,
   }
   
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct LockInfo {
       pub id: String,
       pub operation: String,
       pub who: String,
       pub version: String,
       pub created: DateTime<Utc>,
       pub path: String,
       pub info: String,
   }
   
   impl DynamoDBLock {
       pub async fn acquire(&self) -> Result<LockInfo> {
           let lock_info = LockInfo {
               id: Uuid::new_v4().to_string(),
               operation: "apply".to_string(),
               who: whoami::username(),
               version: env!("CARGO_PKG_VERSION").to_string(),
               created: Utc::now(),
               path: self.key.clone(),
               info: "State lock acquired".to_string(),
           };
           
           let result = self.client
               .put_item()
               .table_name(&self.table)
               .item("LockID", AttributeValue::S(self.key.clone()))
               .item("LockInfo", AttributeValue::S(serde_json::to_string(&lock_info)?))
               .condition_expression("attribute_not_exists(LockID)")
               .send()
               .await;
           
           match result {
               Ok(_) => Ok(lock_info),
               Err(SdkError::ServiceError(err)) if err.err().is_conditional_check_failed_exception() => {
                   Err(Error::StateLocked {
                       message: "State is already locked".to_string(),
                       holder: self.get_current_lock_info().await?,
                   })
               }
               Err(e) => Err(e.into()),
           }
       }
       
       pub async fn release(&self) -> Result<()> {
           self.client
               .delete_item()
               .table_name(&self.table)
               .key("LockID", AttributeValue::S(self.key.clone()))
               .send()
               .await?;
           
           Ok(())
       }
       
       async fn get_current_lock_info(&self) -> Result<LockInfo> {
           let resp = self.client
               .get_item()
               .table_name(&self.table)
               .key("LockID", AttributeValue::S(self.key.clone()))
               .send()
               .await?;
           
           let info_json = resp
               .item()
               .get("LockInfo")
               .and_then(|v| v.as_s().ok())
               .ok_or(Error::InvalidState)?;
           
           let info: LockInfo = serde_json::from_str(info_json)?;
           Ok(info)
       }
   }
   ```

2. **Configuration**
   ```toml
   [state]
   backend = "s3"
   
   [state.s3]
   bucket = "my-rustible-state"
   key = "production/rustible.tfstate"
   
   [state.lock]
   backend = "dynamodb"
   
   [state.lock.dynamodb]
   table = "rustible-locks"
   region = "us-east-1"
   ```

3. **Terraform table schema**
   ```bash
   aws dynamodb create-table \
     --table-name rustible-locks \
     --attribute-definitions AttributeName=LockID,AttributeType=S \
     --key-schema AttributeName=LockID,KeyType=HASH \
     --billing-mode PAY_PER_REQUEST
   ```

### Phase 2: Consul Locking (v0.3.x)
1. **Consul session implementation**
   ```rust
   // src/state/lock/consul.rs
   use consul::{Client as ConsulClient};
   
   pub struct ConsulLock {
       client: ConsulClient,
       key: String,
       session_ttl: Duration,
   }
   
   impl ConsulLock {
       pub async fn acquire(&self) -> Result<LockInfo> {
           // Create Consul session
           let session_id = self.client
               .create_session(&SessionConfig {
                   name: format!("rustible-lock-{}", self.key),
                   ttl: Some(self.session_ttl),
                   behavior: Some("delete".to_string()),
                   ..Default::default()
               })
               .await?;
           
           // Acquire lock using session
           let lock_info = LockInfo {
               id: session_id.clone(),
               operation: "apply".to_string(),
               who: whoami::username(),
               version: env!("CARGO_PKG_VERSION").to_string(),
               created: Utc::now(),
               path: self.key.clone(),
               info: "State lock acquired".to_string(),
           };
           
           let acquired = self.client
               .acquire_lock(&self.key, &session_id)
               .await?;
           
           if !acquired {
               return Err(Error::StateLocked {
                   message: "State is already locked".to_string(),
                   holder: self.get_current_lock_info().await?,
               });
           }
           
           Ok(lock_info)
       }
       
       pub async fn release(&self) -> Result<()> {
           // Destroy session to release lock
           if let Some(session_id) = &self.current_session_id {
               self.client.destroy_session(session_id).await?;
           }
           
           Ok(())
       }
   }
   ```

2. **Configuration**
   ```toml
   [state.lock]
   backend = "consul"
   
   [state.lock.consul]
   address = "localhost:8500"
   key = "rustible/locks/production"
   session_ttl = "30s"
   ```

### Phase 3: Additional Lock Backends (v0.3.x)
1. **PostgreSQL locking**
   ```rust
   // src/state/lock/postgres.rs
   pub struct PostgresLock {
       pool: PgPool,
       key: String,
   }
   
   impl PostgresLock {
       pub async fn acquire(&self) -> Result<LockInfo> {
           let lock_info = LockInfo { /* ... */ };
           
           let result = sqlx::query(
               "INSERT INTO rustible_locks (lock_id, lock_info, expires_at) \
                VALUES ($1, $2, NOW() + INTERVAL '30 minutes') \
                ON CONFLICT (lock_id) DO NOTHING"
           )
           .bind(&self.key)
           .bind(serde_json::to_string(&lock_info)?)
           .execute(&self.pool)
           .await?;
           
           if result.rows_affected() == 0 {
               return Err(Error::StateLocked { /* ... */ });
           }
           
           Ok(lock_info)
       }
   }
   ```

2. **Redis locking**
   ```rust
   // src/state/lock/redis.rs
   pub struct RedisLock {
       client: redis::Client,
       key: String,
   }
   
   impl RedisLock {
       pub async fn acquire(&self) -> Result<LockInfo> {
           let mut conn = self.client.get_async_connection().await?;
           
           let lock_info = LockInfo { /* ... */ };
           
           let acquired: bool = redis::cmd("SET")
               .arg(&self.key)
               .arg(serde_json::to_string(&lock_info)?)
               .arg("NX")
               .arg("EX")
               .arg(1800) // 30 minutes
               .query_async(&mut conn)
               .await?;
           
           if !acquired {
               return Err(Error::StateLocked { /* ... */ });
           }
           
           Ok(lock_info)
       }
   }
   ```

### Phase 4: Lock Management (v0.3.x)
1. **CLI commands**
   ```bash
   # Show lock status
   rustible state lock status -i inventory.yml
   
   # Force unlock (with confirmation)
   rustible state lock force-unlock <lock-id> -i inventory.yml
   
   # Extend lock timeout
   rustible state lock extend -i inventory.yml
   ```

2. **Automatic lock renewal**
   - Extend lock timeout periodically
   - Prevent lock expiration on long-running operations
   - Graceful handling of lock expiration

3. **Lock notifications**
   - Notify when lock is acquired/released
   - Alert on lock timeout
   - Slack/Email notifications

## Expected Outcomes
- Safe concurrent Rustible execution
- Team collaboration support
- CI/CD pipeline coordination
- Lock timeout handling
- Force unlock capability

## Success Criteria
- [ ] DynamoDB locking implemented
- [ ] Consul locking implemented
- [ ] PostgreSQL locking implemented
- [ ] Redis locking implemented
- [ ] Lock acquisition working
- [ ] Lock release working
- [ ] Lock timeout handling
- [ ] Force unlock implemented
- [ ] Lock status command
- [ ] Automatic lock renewal
- [ ] Lock notifications working

## Implementation Details

### Lock Error Handling
```rust
#[derive(Error, Debug)]
pub enum LockError {
    #[error("State is locked by {who} since {created}")]
    Locked {
        message: String,
        holder: LockInfo,
    },
    #[error("Lock acquisition failed: {0}")]
    AcquisitionFailed(String),
    #[error("Lock release failed: {0}")]
    ReleaseFailed(String),
    #[error("Lock expired")]
    Expired,
}
```

### Lock Status Output
```
rustible state lock status -i inventory.yml

State Lock Status: LOCKED
  ID: 550e8400-e29b-41d4-a716-446655440000
  Operation: apply
  Who: john@example.com
  Version: 0.1.0
  Created: 2026-01-27T14:30:00Z
  Age: 5 minutes
  Expires: 2026-01-27T15:00:00Z
  Info: State lock acquired

Commands:
  rustible state lock force-unlock 550e8400-e29b-41d4-a716-446655440000
```

## Related Issues
- #008: State Management
- #009: Remote State Backends
- #011: State Versioning

## Additional Notes
This is a **P1 (High)** feature that enables safe team collaboration. Should be targeted for v0.3.x release.
