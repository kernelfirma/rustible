//! End-to-End Scenario Tests for Rustible
//!
//! These tests simulate real-world deployment scenarios that exercise multiple
//! features together. Each scenario represents a complete deployment workflow
//! that users would typically perform in production environments.
//!
//! All scenarios use local connection for testability.

use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::{Handler, Task};
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test executor for a single host (localhost)
fn create_local_executor(temp_dir: &TempDir) -> Executor {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    // Set common facts for realistic testing
    runtime.set_host_fact(
        "localhost",
        "ansible_os_family".to_string(),
        serde_json::json!("Debian"),
    );
    runtime.set_host_fact(
        "localhost",
        "ansible_distribution".to_string(),
        serde_json::json!("Ubuntu"),
    );
    runtime.set_host_fact(
        "localhost",
        "ansible_distribution_version".to_string(),
        serde_json::json!("22.04"),
    );
    runtime.set_host_fact(
        "localhost",
        "ansible_architecture".to_string(),
        serde_json::json!("x86_64"),
    );
    runtime.set_host_fact(
        "localhost",
        "ansible_processor_cores".to_string(),
        serde_json::json!(4),
    );
    runtime.set_host_fact(
        "localhost",
        "ansible_memtotal_mb".to_string(),
        serde_json::json!(8192),
    );
    runtime.set_host_fact(
        "localhost",
        "temp_dir".to_string(),
        serde_json::json!(temp_dir.path().to_string_lossy()),
    );

    let config = ExecutorConfig {
        forks: 1,
        check_mode: false,
        gather_facts: false,
        ..Default::default()
    };

    Executor::with_runtime(config, runtime)
}

/// Create a multi-host executor for testing distributed scenarios
fn create_multi_host_executor() -> Executor {
    let mut runtime = RuntimeContext::new();

    // Web tier
    runtime.add_host("web1".to_string(), Some("webservers"));
    runtime.add_host("web2".to_string(), Some("webservers"));
    runtime.add_host("web3".to_string(), Some("webservers"));

    // App tier
    runtime.add_host("app1".to_string(), Some("appservers"));
    runtime.add_host("app2".to_string(), Some("appservers"));

    // Database tier
    runtime.add_host("db1".to_string(), Some("databases"));
    runtime.add_host("db2".to_string(), Some("databases"));

    // Load balancer
    runtime.add_host("lb1".to_string(), Some("loadbalancers"));

    // Monitoring
    runtime.add_host("mon1".to_string(), Some("monitoring"));

    // Set facts for all hosts
    for host in [
        "web1", "web2", "web3", "app1", "app2", "db1", "db2", "lb1", "mon1",
    ] {
        runtime.set_host_fact(
            host,
            "ansible_os_family".to_string(),
            serde_json::json!("Debian"),
        );
        runtime.set_host_fact(
            host,
            "ansible_distribution".to_string(),
            serde_json::json!("Ubuntu"),
        );
    }

    let config = ExecutorConfig {
        forks: 5,
        gather_facts: false,
        ..Default::default()
    };

    Executor::with_runtime(config, runtime)
}

// ============================================================================
// SCENARIO 1: Web Server Deployment
// ============================================================================
//
// This scenario simulates deploying a complete web server:
// - Install nginx and PHP-FPM packages
// - Configure nginx with custom settings
// - Deploy website files
// - Set up SSL certificates (simulated)
// - Configure firewall rules
// - Start and enable services
// - Perform health check

#[tokio::test]
async fn test_scenario_web_server_deployment() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Web Server Deployment");

    // Set playbook-level variables
    playbook.set_var("server_name".to_string(), serde_json::json!("example.com"));
    playbook.set_var(
        "document_root".to_string(),
        serde_json::json!(temp_dir.path().join("www").to_string_lossy()),
    );
    playbook.set_var("ssl_enabled".to_string(), serde_json::json!(true));
    playbook.set_var("http_port".to_string(), serde_json::json!(80));
    playbook.set_var("https_port".to_string(), serde_json::json!(443));

    let mut play = Play::new("Deploy Web Server", "localhost");
    play.gather_facts = false;

    // Task 1: Install required packages
    play.add_task(
        Task::new("Install nginx", "package")
            .arg("name", serde_json::json!(["nginx", "php-fpm", "php-mysql"]))
            .arg("state", "present"),
    );

    // Task 2: Create document root directory
    play.add_task(
        Task::new("Create document root", "file")
            .arg(
                "path",
                temp_dir.path().join("www").to_string_lossy().to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o755),
    );

    // Task 3: Create nginx configuration directory
    play.add_task(
        Task::new("Create nginx config directory", "file")
            .arg(
                "path",
                temp_dir.path().join("nginx").to_string_lossy().to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o755),
    );

    // Task 4: Deploy nginx main configuration
    play.add_task(
        Task::new("Deploy nginx main config", "copy")
            .arg(
                "content",
                r#"
user www-data;
worker_processes auto;
pid /run/nginx.pid;

events {
    worker_connections 1024;
}

http {
    include mime.types;
    default_type application/octet-stream;
    sendfile on;
    keepalive_timeout 65;

    include /etc/nginx/conf.d/*.conf;
}
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("nginx/nginx.conf")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("reload nginx"),
    );

    // Task 5: Deploy virtual host configuration
    play.add_task(
        Task::new("Deploy virtual host config", "copy")
            .arg(
                "content",
                r#"
server {
    listen 80;
    server_name example.com;
    root /var/www/html;
    index index.php index.html;

    location / {
        try_files $uri $uri/ =404;
    }

    location ~ \.php$ {
        fastcgi_pass unix:/run/php/php-fpm.sock;
        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
        include fastcgi_params;
    }
}
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("nginx/default.conf")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("reload nginx"),
    );

    // Task 6: Deploy website files
    play.add_task(
        Task::new("Deploy index.html", "copy")
            .arg(
                "content",
                r#"<!DOCTYPE html>
<html>
<head><title>Welcome to {{ server_name }}</title></head>
<body>
<h1>Success! The web server is configured.</h1>
</body>
</html>
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("www/index.html")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 7: Create SSL directory structure (simulated)
    play.add_task(
        Task::new("Create SSL certificate directory", "file")
            .arg(
                "path",
                temp_dir.path().join("ssl").to_string_lossy().to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700)
            .when("ssl_enabled"),
    );

    // Task 8: Create self-signed certificate placeholder
    play.add_task(
        Task::new("Create SSL certificate placeholder", "copy")
            .arg(
                "content",
                "-----BEGIN CERTIFICATE-----\nSimulated SSL Certificate\n-----END CERTIFICATE-----",
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("ssl/server.crt")
                    .to_string_lossy()
                    .to_string(),
            )
            .when("ssl_enabled"),
    );

    // Task 9: Create firewall rules file
    play.add_task(
        Task::new("Configure firewall rules", "copy")
            .arg(
                "content",
                r#"
# HTTP/HTTPS rules
-A INPUT -p tcp --dport 80 -j ACCEPT
-A INPUT -p tcp --dport 443 -j ACCEPT
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("firewall.rules")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 10: Start nginx service
    play.add_task(
        Task::new("Start nginx", "service")
            .arg("name", "nginx")
            .arg("state", "started")
            .arg("enabled", true),
    );

    // Task 11: Start PHP-FPM service
    play.add_task(
        Task::new("Start PHP-FPM", "service")
            .arg("name", "php-fpm")
            .arg("state", "started")
            .arg("enabled", true),
    );

    // Task 12: Health check - verify config file exists
    play.add_task(Task::new("Verify deployment", "debug").arg(
        "msg",
        "Web server deployment completed for {{ server_name }}",
    ));

    // Handler for nginx reload
    play.add_handler(Handler {
        name: "reload nginx".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("nginx"));
            args.insert("state".to_string(), serde_json::json!("reloaded"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify execution succeeded
    let localhost_result = results.get("localhost").unwrap();
    assert!(
        !localhost_result.failed,
        "Web server deployment should succeed"
    );
    assert!(localhost_result.stats.changed > 0 || localhost_result.stats.ok > 0);

    // Verify files were created
    assert!(temp_dir.path().join("www").exists());
    assert!(temp_dir.path().join("www/index.html").exists());
    assert!(temp_dir.path().join("nginx/nginx.conf").exists());
    assert!(temp_dir.path().join("nginx/default.conf").exists());
    assert!(temp_dir.path().join("ssl").exists());
    assert!(temp_dir.path().join("firewall.rules").exists());
}

// ============================================================================
// SCENARIO 2: Database Setup
// ============================================================================
//
// This scenario simulates setting up a database server:
// - Install database packages (PostgreSQL)
// - Configure database settings
// - Create database users
// - Set proper permissions
// - Initialize database schema
// - Configure backup procedures

#[tokio::test]
async fn test_scenario_database_setup() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Database Server Setup");

    playbook.set_var("db_name".to_string(), serde_json::json!("appdb"));
    playbook.set_var("db_user".to_string(), serde_json::json!("appuser"));
    playbook.set_var(
        "db_password".to_string(),
        serde_json::json!("secure_password_123"),
    );
    playbook.set_var("db_port".to_string(), serde_json::json!(5432));
    playbook.set_var("backup_enabled".to_string(), serde_json::json!(true));
    playbook.set_var("backup_retention_days".to_string(), serde_json::json!(7));

    let mut play = Play::new("Setup PostgreSQL Database", "localhost");
    play.gather_facts = false;

    // Task 1: Install PostgreSQL packages
    play.add_task(
        Task::new("Install PostgreSQL", "package")
            .arg(
                "name",
                serde_json::json!(["postgresql", "postgresql-contrib", "python3-psycopg2"]),
            )
            .arg("state", "present"),
    );

    // Task 2: Create data directory
    play.add_task(
        Task::new("Create PostgreSQL data directory", "file")
            .arg(
                "path",
                temp_dir.path().join("pgdata").to_string_lossy().to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700),
    );

    // Task 3: Create configuration directory
    play.add_task(
        Task::new("Create config directory", "file")
            .arg(
                "path",
                temp_dir.path().join("pgsql").to_string_lossy().to_string(),
            )
            .arg("state", "directory"),
    );

    // Task 4: Deploy postgresql.conf
    play.add_task(
        Task::new("Deploy PostgreSQL configuration", "copy")
            .arg(
                "content",
                r#"
# PostgreSQL Configuration
listen_addresses = 'localhost'
port = 5432
max_connections = 100
shared_buffers = 256MB
effective_cache_size = 768MB
maintenance_work_mem = 64MB
checkpoint_completion_target = 0.9
wal_buffers = 7864kB
default_statistics_target = 100
random_page_cost = 1.1
effective_io_concurrency = 200
work_mem = 1310kB
min_wal_size = 1GB
max_wal_size = 4GB

# Logging
logging_collector = on
log_directory = 'log'
log_filename = 'postgresql-%Y-%m-%d_%H%M%S.log'
log_min_duration_statement = 1000
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("pgsql/postgresql.conf")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("restart postgresql"),
    );

    // Task 5: Deploy pg_hba.conf (authentication rules)
    play.add_task(
        Task::new("Deploy pg_hba.conf", "copy")
            .arg(
                "content",
                r#"
# PostgreSQL Client Authentication Configuration
# TYPE  DATABASE        USER            ADDRESS                 METHOD
local   all             postgres                                peer
local   all             all                                     peer
host    all             all             127.0.0.1/32            scram-sha-256
host    all             all             ::1/128                 scram-sha-256
host    appdb           appuser         10.0.0.0/8              scram-sha-256
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("pgsql/pg_hba.conf")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("reload postgresql"),
    );

    // Task 6: Create SQL initialization script
    play.add_task(
        Task::new("Create database initialization script", "copy")
            .arg(
                "content",
                r#"
-- Create application database
CREATE DATABASE appdb WITH ENCODING 'UTF8';

-- Create application user
CREATE USER appuser WITH ENCRYPTED PASSWORD 'secure_password_123';

-- Grant privileges
GRANT ALL PRIVILEGES ON DATABASE appdb TO appuser;

-- Connect to app database and create schema
\c appdb

-- Create tables
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(100) UNIQUE NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sessions (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    token VARCHAR(255) NOT NULL,
    expires_at TIMESTAMP NOT NULL
);

-- Create indexes
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_sessions_token ON sessions(token);
CREATE INDEX idx_sessions_user_id ON sessions(user_id);
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("pgsql/init.sql")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 7: Create backup script
    play.add_task(
        Task::new("Create backup script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# PostgreSQL Backup Script
BACKUP_DIR="/var/backups/postgresql"
DATE=$(date +%Y%m%d_%H%M%S)
RETENTION_DAYS=7

# Create backup directory if it doesn't exist
mkdir -p "$BACKUP_DIR"

# Backup all databases
pg_dumpall -U postgres | gzip > "$BACKUP_DIR/full_backup_$DATE.sql.gz"

# Remove old backups
find "$BACKUP_DIR" -name "*.sql.gz" -mtime +$RETENTION_DAYS -delete

echo "Backup completed: $DATE"
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("pgsql/backup.sh")
                    .to_string_lossy()
                    .to_string(),
            )
            .when("backup_enabled"),
    );

    // Task 8: Create backup cron file
    play.add_task(
        Task::new("Create backup cron job", "copy")
            .arg(
                "content",
                "0 2 * * * postgres /opt/pgsql/backup.sh >> /var/log/pg_backup.log 2>&1\n",
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("pgsql/backup.cron")
                    .to_string_lossy()
                    .to_string(),
            )
            .when("backup_enabled"),
    );

    // Task 9: Create log directory
    play.add_task(
        Task::new("Create log directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("pgsql/log")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o755),
    );

    // Task 10: Start PostgreSQL service
    play.add_task(
        Task::new("Start PostgreSQL", "service")
            .arg("name", "postgresql")
            .arg("state", "started")
            .arg("enabled", true),
    );

    // Task 11: Verification
    play.add_task(Task::new("Database setup complete", "debug").arg(
        "msg",
        "PostgreSQL setup complete: database={{ db_name }}, user={{ db_user }}",
    ));

    // Handlers
    play.add_handler(Handler {
        name: "restart postgresql".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("postgresql"));
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    play.add_handler(Handler {
        name: "reload postgresql".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("postgresql"));
            args.insert("state".to_string(), serde_json::json!("reloaded"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify execution
    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed, "Database setup should succeed");

    // Verify files
    assert!(temp_dir.path().join("pgdata").exists());
    assert!(temp_dir.path().join("pgsql/postgresql.conf").exists());
    assert!(temp_dir.path().join("pgsql/pg_hba.conf").exists());
    assert!(temp_dir.path().join("pgsql/init.sql").exists());
    assert!(temp_dir.path().join("pgsql/backup.sh").exists());
    assert!(temp_dir.path().join("pgsql/backup.cron").exists());
}

// ============================================================================
// SCENARIO 3: Application Deployment
// ============================================================================
//
// This scenario simulates a complete application deployment:
// - Clone/pull code from git (simulated)
// - Install application dependencies
// - Configure environment variables
// - Run database migrations
// - Restart application services
// - Simulate blue/green deployment

#[tokio::test]
async fn test_scenario_application_deployment() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Application Deployment");

    playbook.set_var("app_name".to_string(), serde_json::json!("myapp"));
    playbook.set_var("app_version".to_string(), serde_json::json!("2.5.0"));
    playbook.set_var("app_env".to_string(), serde_json::json!("production"));
    playbook.set_var(
        "git_repo".to_string(),
        serde_json::json!("https://github.com/example/myapp.git"),
    );
    playbook.set_var("deploy_user".to_string(), serde_json::json!("deploy"));
    playbook.set_var("blue_green_enabled".to_string(), serde_json::json!(true));

    let mut play = Play::new("Deploy Application", "localhost");
    play.gather_facts = false;

    // Set play variables
    play.set_var(
        "app_base".to_string(),
        serde_json::json!(temp_dir.path().join("apps").to_string_lossy()),
    );
    play.set_var("current_slot".to_string(), serde_json::json!("blue"));
    play.set_var("next_slot".to_string(), serde_json::json!("green"));

    // Task 1: Create application directories
    play.add_task(
        Task::new("Create application base directory", "file")
            .arg(
                "path",
                temp_dir.path().join("apps").to_string_lossy().to_string(),
            )
            .arg("state", "directory"),
    );

    // Task 2: Create blue slot directory
    play.add_task(
        Task::new("Create blue deployment slot", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("apps/blue")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .when("blue_green_enabled"),
    );

    // Task 3: Create green slot directory
    play.add_task(
        Task::new("Create green deployment slot", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("apps/green")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .when("blue_green_enabled"),
    );

    // Task 4: Simulate git clone (create app structure)
    play.add_task(
        Task::new("Pull application code", "debug")
            .arg("msg", "Simulating git clone from {{ git_repo }}"),
    );

    // Task 5: Create simulated app files
    play.add_task(
        Task::new("Create application files", "copy")
            .arg(
                "content",
                r#"#!/usr/bin/env python3
# Application entry point
import os
print(f"Starting {os.environ.get('APP_NAME', 'app')} v{os.environ.get('APP_VERSION', 'unknown')}")
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("apps/green/app.py")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 6: Create requirements.txt
    play.add_task(
        Task::new("Create requirements file", "copy")
            .arg(
                "content",
                r#"flask==2.3.0
gunicorn==21.2.0
psycopg2-binary==2.9.9
redis==5.0.1
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("apps/green/requirements.txt")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 7: Install dependencies (simulated)
    play.add_task(
        Task::new("Install Python dependencies", "debug")
            .arg("msg", "Installing dependencies from requirements.txt"),
    );

    // Task 8: Create environment configuration
    play.add_task(
        Task::new("Create environment file", "copy")
            .arg(
                "content",
                r#"# Application Environment Configuration
APP_NAME=myapp
APP_VERSION=2.5.0
APP_ENV=production
DATABASE_URL=postgresql://appuser:secure@localhost:5432/appdb
REDIS_URL=redis://localhost:6379/0
SECRET_KEY=production_secret_key_here
LOG_LEVEL=INFO
DEBUG=false
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("apps/green/.env")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 9: Run database migrations (simulated)
    play.add_task(
        Task::new("Create migration script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Database migration script
echo "Running database migrations for version $APP_VERSION"
# flask db upgrade
echo "Migrations complete"
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("apps/green/migrate.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    play.add_task(
        Task::new("Run database migrations", "debug")
            .arg("msg", "Running migrations for version {{ app_version }}"),
    );

    // Task 10: Create systemd service file
    play.add_task(
        Task::new("Create systemd service file", "copy")
            .arg(
                "content",
                r#"[Unit]
Description=MyApp Application Service
After=network.target postgresql.service redis.service

[Service]
Type=notify
User=deploy
Group=deploy
WorkingDirectory=/opt/apps/current
Environment="PATH=/opt/apps/current/venv/bin"
EnvironmentFile=/opt/apps/current/.env
ExecStart=/opt/apps/current/venv/bin/gunicorn -w 4 -b 0.0.0.0:8000 app:app
ExecReload=/bin/kill -s HUP $MAINPID
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("apps/myapp.service")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("reload systemd"),
    );

    // Task 11: Switch to new deployment (update symlink)
    play.add_task(
        Task::new("Update current symlink", "debug")
            .arg(
                "msg",
                "Switching from {{ current_slot }} to {{ next_slot }}",
            )
            .when("blue_green_enabled"),
    );

    // Task 12: Restart application service
    play.add_task(
        Task::new("Restart application", "service")
            .arg("name", "myapp")
            .arg("state", "restarted"),
    );

    // Task 13: Health check
    play.add_task(Task::new("Wait for application to start", "pause").arg("seconds", 1));

    play.add_task(Task::new("Application health check", "debug").arg(
        "msg",
        "Application {{ app_name }} v{{ app_version }} deployed successfully to {{ app_env }}",
    ));

    // Handlers
    play.add_handler(Handler {
        name: "reload systemd".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert(
                "msg".to_string(),
                serde_json::json!("Reloading systemd daemon"),
            );
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(
        !localhost_result.failed,
        "Application deployment should succeed"
    );

    // Verify deployment artifacts
    assert!(temp_dir.path().join("apps").exists());
    assert!(temp_dir.path().join("apps/blue").exists());
    assert!(temp_dir.path().join("apps/green").exists());
    assert!(temp_dir.path().join("apps/green/app.py").exists());
    assert!(temp_dir.path().join("apps/green/.env").exists());
    assert!(temp_dir.path().join("apps/myapp.service").exists());
}

// ============================================================================
// SCENARIO 4: User Management
// ============================================================================
//
// This scenario simulates complete user management:
// - Create multiple users
// - Set up SSH keys for each user
// - Configure sudo access
// - Set passwords (simulated)
// - Create home directories with proper structure
// - Set default shells

#[tokio::test]
async fn test_scenario_user_management() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("User Management");

    // Define users to create
    playbook.set_var(
        "users".to_string(),
        serde_json::json!([
            {
                "name": "alice",
                "uid": 1001,
                "groups": ["developers", "docker"],
                "shell": "/bin/bash",
                "sudo": true
            },
            {
                "name": "bob",
                "uid": 1002,
                "groups": ["developers"],
                "shell": "/bin/bash",
                "sudo": false
            },
            {
                "name": "charlie",
                "uid": 1003,
                "groups": ["operations", "docker"],
                "shell": "/bin/zsh",
                "sudo": true
            }
        ]),
    );

    let mut play = Play::new("Setup Users", "localhost");
    play.gather_facts = false;

    // Task 1: Create base home directory
    play.add_task(
        Task::new("Create home directory base", "file")
            .arg(
                "path",
                temp_dir.path().join("home").to_string_lossy().to_string(),
            )
            .arg("state", "directory"),
    );

    // Task 2: Create groups directory (for group configs)
    play.add_task(
        Task::new("Create sudoers.d directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("sudoers.d")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o750),
    );

    // Create directories and files for user alice
    play.add_task(
        Task::new("Create home for alice", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("home/alice")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700),
    );

    play.add_task(
        Task::new("Create .ssh for alice", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("home/alice/.ssh")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700),
    );

    play.add_task(
        Task::new("Deploy SSH key for alice", "copy")
            .arg("content", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM2Cj4XKLq8tRf1lB6T5vR8mM4qR5sZ7yF2aB3cD4eF5 alice@example.com\n")
            .arg("dest", temp_dir.path().join("home/alice/.ssh/authorized_keys").to_string_lossy().to_string())
            .arg("mode", 0o600)
    );

    play.add_task(
        Task::new("Create bashrc for alice", "copy")
            .arg(
                "content",
                r#"# Alice's bashrc
export PATH="$HOME/bin:$PATH"
export EDITOR=vim
alias ll='ls -la'
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("home/alice/.bashrc")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    play.add_task(
        Task::new("Create sudoers entry for alice", "copy")
            .arg("content", "alice ALL=(ALL) NOPASSWD: ALL\n")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("sudoers.d/alice")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("mode", 0o440),
    );

    // Create directories and files for user bob
    play.add_task(
        Task::new("Create home for bob", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("home/bob")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700),
    );

    play.add_task(
        Task::new("Create .ssh for bob", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("home/bob/.ssh")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700),
    );

    play.add_task(
        Task::new("Deploy SSH key for bob", "copy")
            .arg(
                "content",
                "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC... bob@example.com\n",
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("home/bob/.ssh/authorized_keys")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("mode", 0o600),
    );

    // Create directories and files for user charlie
    play.add_task(
        Task::new("Create home for charlie", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("home/charlie")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700),
    );

    play.add_task(
        Task::new("Create .ssh for charlie", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("home/charlie/.ssh")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o700),
    );

    play.add_task(
        Task::new("Deploy SSH key for charlie", "copy")
            .arg("content", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG5tH7sK8lM9nO2pQ3rS5tU6vW8xY9zA1bC2dE3fG4hI charlie@example.com\n")
            .arg("dest", temp_dir.path().join("home/charlie/.ssh/authorized_keys").to_string_lossy().to_string())
            .arg("mode", 0o600)
    );

    play.add_task(
        Task::new("Create zshrc for charlie", "copy")
            .arg(
                "content",
                r#"# Charlie's zshrc
export PATH="$HOME/bin:$PATH"
export EDITOR=vim
alias ll='ls -la'
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("home/charlie/.zshrc")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    play.add_task(
        Task::new("Create sudoers entry for charlie", "copy")
            .arg("content", "charlie ALL=(ALL) NOPASSWD: ALL\n")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("sudoers.d/charlie")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("mode", 0o440),
    );

    // User summary
    play.add_task(Task::new("User management complete", "debug").arg(
        "msg",
        "Created users: alice, bob, charlie with SSH keys and sudo access",
    ));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed, "User management should succeed");

    // Verify user directories
    for user in ["alice", "bob", "charlie"] {
        let home = temp_dir.path().join("home").join(user);
        assert!(home.exists(), "Home for {} should exist", user);
        assert!(home.join(".ssh").exists(), ".ssh for {} should exist", user);
        assert!(
            home.join(".ssh/authorized_keys").exists(),
            "authorized_keys for {} should exist",
            user
        );
    }

    // Verify sudoers entries
    assert!(temp_dir.path().join("sudoers.d/alice").exists());
    assert!(temp_dir.path().join("sudoers.d/charlie").exists());
    assert!(!temp_dir.path().join("sudoers.d/bob").exists()); // bob doesn't have sudo
}

// ============================================================================
// SCENARIO 5: Monitoring Setup
// ============================================================================
//
// This scenario simulates setting up monitoring infrastructure:
// - Install monitoring agent (Prometheus node exporter)
// - Configure monitoring endpoints
// - Set up alerting rules
// - Test connectivity to monitoring server
// - Verify metrics collection

#[tokio::test]
async fn test_scenario_monitoring_setup() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Monitoring Setup");

    playbook.set_var(
        "monitoring_server".to_string(),
        serde_json::json!("prometheus.example.com"),
    );
    playbook.set_var("node_exporter_port".to_string(), serde_json::json!(9100));
    playbook.set_var("alertmanager_enabled".to_string(), serde_json::json!(true));
    playbook.set_var("scrape_interval".to_string(), serde_json::json!("15s"));

    let mut play = Play::new("Configure Monitoring", "localhost");
    play.gather_facts = false;

    // Task 1: Create monitoring directories
    play.add_task(
        Task::new("Create monitoring directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("monitoring")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    play.add_task(
        Task::new("Create node_exporter directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("monitoring/node_exporter")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    // Task 2: Install node exporter (simulated)
    play.add_task(
        Task::new("Install node exporter", "package")
            .arg("name", "prometheus-node-exporter")
            .arg("state", "present"),
    );

    // Task 3: Create node exporter configuration
    play.add_task(
        Task::new("Configure node exporter", "copy")
            .arg(
                "content",
                r#"# Node Exporter Configuration
ARGS="--web.listen-address=:9100 \
      --collector.systemd \
      --collector.processes \
      --collector.filesystem.ignored-mount-points='^/(sys|proc|dev|host|etc)($|/)'"
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("monitoring/node_exporter/config")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("restart node_exporter"),
    );

    // Task 4: Create systemd service for node exporter
    play.add_task(
        Task::new("Create node exporter service file", "copy")
            .arg(
                "content",
                r#"[Unit]
Description=Prometheus Node Exporter
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=node_exporter
Group=node_exporter
EnvironmentFile=/etc/default/node_exporter
ExecStart=/usr/bin/prometheus-node-exporter $ARGS
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("monitoring/node_exporter.service")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 5: Create alerting rules
    // Note: Content uses Prometheus template syntax which doesn't contain {{ }} to avoid
    // triggering Jinja2 template processing in the copy module
    play.add_task(
        Task::new("Create alert rules", "copy")
            .arg(
                "content",
                r#"groups:
  - name: node_alerts
    rules:
      - alert: HighCPUUsage
        expr: 100 - (avg by(instance) (irate(node_cpu_seconds_total{mode="idle"}[5m])) * 100) > 80
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: High CPU usage detected
          description: CPU usage is above 80% for 5 minutes

      - alert: HighMemoryUsage
        expr: (1 - (node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes)) * 100 > 90
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: High memory usage detected
          description: Memory usage is above 90% for 5 minutes

      - alert: DiskSpaceLow
        expr: (1 - (node_filesystem_avail_bytes / node_filesystem_size_bytes)) * 100 > 85
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: Low disk space detected
          description: Disk usage is above 85%

      - alert: HostDown
        expr: up == 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: Host is down
          description: Host has been unreachable for more than 1 minute
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("monitoring/alert_rules.yml")
                    .to_string_lossy()
                    .to_string(),
            )
            .when("alertmanager_enabled"),
    );

    // Task 6: Create Prometheus scrape config
    play.add_task(
        Task::new("Create Prometheus target config", "copy")
            .arg(
                "content",
                r#"# Prometheus scrape configuration for this host
- targets:
    - localhost:9100
  labels:
    job: node
    instance: localhost
    env: production
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("monitoring/prometheus_targets.yml")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 7: Create health check script
    play.add_task(
        Task::new("Create health check script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Monitoring health check script

# Check node exporter
if curl -s http://localhost:9100/metrics > /dev/null; then
    echo "Node exporter: OK"
else
    echo "Node exporter: FAILED"
    exit 1
fi

# Check key metrics availability
metrics_count=$(curl -s http://localhost:9100/metrics | wc -l)
if [ $metrics_count -gt 100 ]; then
    echo "Metrics collection: OK ($metrics_count metrics)"
else
    echo "Metrics collection: LOW ($metrics_count metrics)"
fi

echo "Health check passed"
exit 0
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("monitoring/healthcheck.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 8: Start node exporter
    play.add_task(
        Task::new("Start node exporter", "service")
            .arg("name", "prometheus-node-exporter")
            .arg("state", "started")
            .arg("enabled", true),
    );

    // Task 9: Verify metrics endpoint (simulated)
    play.add_task(Task::new("Verify metrics endpoint", "debug").arg(
        "msg",
        "Monitoring configured on port {{ node_exporter_port }}",
    ));

    // Task 10: Test connectivity to monitoring server
    play.add_task(
        Task::new("Test monitoring server connectivity", "debug")
            .arg("msg", "Testing connection to {{ monitoring_server }}"),
    );

    // Handler
    play.add_handler(Handler {
        name: "restart node_exporter".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert(
                "name".to_string(),
                serde_json::json!("prometheus-node-exporter"),
            );
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed, "Monitoring setup should succeed");

    // Verify monitoring files
    assert!(temp_dir
        .path()
        .join("monitoring/node_exporter/config")
        .exists());
    assert!(temp_dir
        .path()
        .join("monitoring/node_exporter.service")
        .exists());
    assert!(temp_dir.path().join("monitoring/alert_rules.yml").exists());
    assert!(temp_dir
        .path()
        .join("monitoring/prometheus_targets.yml")
        .exists());
    assert!(temp_dir.path().join("monitoring/healthcheck.sh").exists());
}

// ============================================================================
// SCENARIO 6: Security Hardening
// ============================================================================
//
// This scenario simulates security hardening:
// - Configure SSH hardening
// - Set up firewall rules
// - Disable unnecessary services
// - Configure AppArmor/SELinux (simulated)
// - Audit configuration

#[tokio::test]
async fn test_scenario_security_hardening() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Security Hardening");

    playbook.set_var("ssh_port".to_string(), serde_json::json!(22022));
    playbook.set_var("disable_root_login".to_string(), serde_json::json!(true));
    playbook.set_var("password_auth".to_string(), serde_json::json!(false));
    playbook.set_var(
        "allowed_users".to_string(),
        serde_json::json!(["admin", "deploy"]),
    );

    let mut play = Play::new("Harden System Security", "localhost");
    play.gather_facts = false;

    // Task 1: Create security config directories
    play.add_task(
        Task::new("Create SSH config directory", "file")
            .arg(
                "path",
                temp_dir.path().join("ssh").to_string_lossy().to_string(),
            )
            .arg("state", "directory")
            .arg("mode", 0o755),
    );

    play.add_task(
        Task::new("Create firewall config directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("firewall")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    play.add_task(
        Task::new("Create audit config directory", "file")
            .arg(
                "path",
                temp_dir.path().join("audit").to_string_lossy().to_string(),
            )
            .arg("state", "directory"),
    );

    // Task 2: Configure SSH hardening
    play.add_task(
        Task::new("Deploy hardened SSH config", "copy")
            .arg(
                "content",
                r#"# SSH Server Hardening Configuration
Port 22022
Protocol 2
HostKey /etc/ssh/ssh_host_ed25519_key
HostKey /etc/ssh/ssh_host_rsa_key

# Authentication
PermitRootLogin no
PasswordAuthentication no
PubkeyAuthentication yes
PermitEmptyPasswords no
ChallengeResponseAuthentication no
UsePAM yes

# Session
X11Forwarding no
PrintMotd no
PrintLastLog yes
TCPKeepAlive yes
ClientAliveInterval 300
ClientAliveCountMax 2
MaxAuthTries 3
MaxSessions 5

# Security
AllowAgentForwarding no
AllowTcpForwarding no
PermitTunnel no
StrictModes yes

# Allow only specific users
AllowUsers admin deploy

# Logging
SyslogFacility AUTH
LogLevel VERBOSE

# Key exchange and ciphers (strong only)
KexAlgorithms curve25519-sha256@libssh.org,diffie-hellman-group-exchange-sha256
Ciphers chacha20-poly1305@openssh.com,aes256-gcm@openssh.com,aes128-gcm@openssh.com
MACs hmac-sha2-512-etm@openssh.com,hmac-sha2-256-etm@openssh.com
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("ssh/sshd_config")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("restart sshd"),
    );

    // Task 3: Create SSH banner
    play.add_task(
        Task::new("Create SSH banner", "copy")
            .arg(
                "content",
                r#"
*******************************************************************
*                         AUTHORIZED USE ONLY                      *
*                                                                   *
*  This system is for authorized users only. All activity may be   *
*  monitored and recorded. Unauthorized access will be prosecuted. *
*                                                                   *
*******************************************************************
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("ssh/banner")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 4: Configure firewall rules (iptables style)
    play.add_task(
        Task::new("Deploy firewall rules", "copy")
            .arg("content", r#"#!/bin/bash
# Firewall Rules - Security Hardened

# Flush existing rules
iptables -F
iptables -X

# Default policies
iptables -P INPUT DROP
iptables -P FORWARD DROP
iptables -P OUTPUT ACCEPT

# Allow loopback
iptables -A INPUT -i lo -j ACCEPT
iptables -A OUTPUT -o lo -j ACCEPT

# Allow established connections
iptables -A INPUT -m state --state ESTABLISHED,RELATED -j ACCEPT

# Allow SSH (custom port)
iptables -A INPUT -p tcp --dport 22022 -m state --state NEW -j ACCEPT

# Allow HTTP/HTTPS
iptables -A INPUT -p tcp --dport 80 -m state --state NEW -j ACCEPT
iptables -A INPUT -p tcp --dport 443 -m state --state NEW -j ACCEPT

# Allow monitoring
iptables -A INPUT -p tcp --dport 9100 -s 10.0.0.0/8 -j ACCEPT

# Rate limit for brute force protection
iptables -A INPUT -p tcp --dport 22022 -m state --state NEW -m recent --set
iptables -A INPUT -p tcp --dport 22022 -m state --state NEW -m recent --update --seconds 60 --hitcount 4 -j DROP

# Log dropped packets
iptables -A INPUT -j LOG --log-prefix "IPTables-Dropped: " --log-level 4
iptables -A INPUT -j DROP

echo "Firewall rules applied"
"#)
            .arg("dest", temp_dir.path().join("firewall/rules.sh").to_string_lossy().to_string())
    );

    // Task 5: Configure fail2ban
    play.add_task(
        Task::new("Deploy fail2ban SSH config", "copy")
            .arg(
                "content",
                r#"[sshd]
enabled = true
port = 22022
filter = sshd
logpath = /var/log/auth.log
maxretry = 3
findtime = 600
bantime = 3600
action = iptables-allports[name=ssh]
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("firewall/fail2ban-sshd.conf")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 6: Create list of services to disable
    play.add_task(
        Task::new("Create services disable list", "copy")
            .arg(
                "content",
                r#"# Services to disable for security hardening
avahi-daemon
cups
cups-browsed
bluetooth
rpcbind
nfs-common
rsh.socket
rlogin.socket
rexec.socket
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("firewall/disable_services.txt")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 7: Disable unnecessary services (simulated)
    play.add_task(Task::new("Disable unnecessary services", "debug").arg(
        "msg",
        "Disabling unnecessary services as per security policy",
    ));

    // Task 8: Configure sysctl security settings
    play.add_task(
        Task::new("Deploy sysctl security settings", "copy")
            .arg(
                "content",
                r#"# Security hardening sysctl settings

# Disable IP forwarding
net.ipv4.ip_forward = 0
net.ipv6.conf.all.forwarding = 0

# Disable source routing
net.ipv4.conf.all.accept_source_route = 0
net.ipv6.conf.all.accept_source_route = 0

# Enable SYN cookies
net.ipv4.tcp_syncookies = 1

# Disable ICMP redirects
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv6.conf.all.accept_redirects = 0

# Disable send redirects
net.ipv4.conf.all.send_redirects = 0
net.ipv4.conf.default.send_redirects = 0

# Log martian packets
net.ipv4.conf.all.log_martians = 1
net.ipv4.conf.default.log_martians = 1

# Ignore ICMP broadcasts
net.ipv4.icmp_echo_ignore_broadcasts = 1

# Ignore bogus ICMP responses
net.ipv4.icmp_ignore_bogus_error_responses = 1

# Randomize virtual address space
kernel.randomize_va_space = 2

# Restrict core dumps
fs.suid_dumpable = 0

# Restrict kernel pointer access
kernel.kptr_restrict = 2
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("firewall/99-security.conf")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 9: Configure audit rules
    play.add_task(
        Task::new("Deploy audit rules", "copy")
            .arg(
                "content",
                r#"# Audit rules for security monitoring

# Monitor changes to user/group files
-w /etc/passwd -p wa -k identity
-w /etc/group -p wa -k identity
-w /etc/shadow -p wa -k identity
-w /etc/gshadow -p wa -k identity

# Monitor SSH configuration
-w /etc/ssh/sshd_config -p wa -k sshd
-w /etc/ssh/sshd_config.d -p wa -k sshd

# Monitor sudo usage
-w /etc/sudoers -p wa -k sudoers
-w /etc/sudoers.d -p wa -k sudoers

# Monitor cron
-w /etc/crontab -p wa -k cron
-w /etc/cron.d -p wa -k cron

# Monitor system calls for privilege escalation
-a exit,always -F arch=b64 -S execve -F uid=0 -F auid>=1000 -k privileged

# Monitor file deletions
-a always,exit -F arch=b64 -S unlink -S unlinkat -S rename -S renameat -k delete

# Monitor network configuration changes
-w /etc/hosts -p wa -k network
-w /etc/network -p wa -k network
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("audit/audit.rules")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 10: Create security audit script
    play.add_task(
        Task::new("Create security audit script", "copy")
            .arg("content", r#"#!/bin/bash
# Security Audit Script

echo "=== Security Audit Report ==="
echo "Date: $(date)"
echo ""

# Check SSH configuration
echo "=== SSH Configuration ==="
grep -E "^(PermitRootLogin|PasswordAuthentication|Port)" /etc/ssh/sshd_config 2>/dev/null || echo "SSH config not found"
echo ""

# Check open ports
echo "=== Open Ports ==="
ss -tlnp 2>/dev/null | head -20
echo ""

# Check failed login attempts
echo "=== Failed Login Attempts (last 24h) ==="
grep "Failed password" /var/log/auth.log 2>/dev/null | tail -10 || echo "No failed attempts found"
echo ""

# Check sudo usage
echo "=== Recent Sudo Usage ==="
grep "sudo:" /var/log/auth.log 2>/dev/null | tail -10 || echo "No sudo usage found"
echo ""

echo "=== Audit Complete ==="
"#)
            .arg("dest", temp_dir.path().join("audit/security_audit.sh").to_string_lossy().to_string())
    );

    // Task 11: Apply security settings (simulated)
    play.add_task(Task::new("Apply security hardening", "debug").arg(
        "msg",
        "Security hardening applied: SSH on port {{ ssh_port }}, root login disabled",
    ));

    // Handler
    play.add_handler(Handler {
        name: "restart sshd".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("sshd"));
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(
        !localhost_result.failed,
        "Security hardening should succeed"
    );

    // Verify security files
    assert!(temp_dir.path().join("ssh/sshd_config").exists());
    assert!(temp_dir.path().join("ssh/banner").exists());
    assert!(temp_dir.path().join("firewall/rules.sh").exists());
    assert!(temp_dir.path().join("firewall/fail2ban-sshd.conf").exists());
    assert!(temp_dir.path().join("firewall/99-security.conf").exists());
    assert!(temp_dir.path().join("audit/audit.rules").exists());

    // Verify SSH config content
    let ssh_config = fs::read_to_string(temp_dir.path().join("ssh/sshd_config")).unwrap();
    assert!(ssh_config.contains("PermitRootLogin no"));
    assert!(ssh_config.contains("PasswordAuthentication no"));
}

// ============================================================================
// SCENARIO 7: Container Deployment
// ============================================================================
//
// This scenario simulates container deployment with Docker:
// - Pull container images
// - Create container networks
// - Deploy application containers
// - Configure volumes
// - Set up health checks

#[tokio::test]
async fn test_scenario_container_deployment() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Container Deployment");

    playbook.set_var("app_image".to_string(), serde_json::json!("myapp:latest"));
    playbook.set_var(
        "redis_image".to_string(),
        serde_json::json!("redis:7-alpine"),
    );
    playbook.set_var(
        "postgres_image".to_string(),
        serde_json::json!("postgres:15-alpine"),
    );
    playbook.set_var("network_name".to_string(), serde_json::json!("app_network"));

    let mut play = Play::new("Deploy Containers", "localhost");
    play.gather_facts = false;

    // Task 1: Create docker directory structure
    play.add_task(
        Task::new("Create docker directory", "file")
            .arg(
                "path",
                temp_dir.path().join("docker").to_string_lossy().to_string(),
            )
            .arg("state", "directory"),
    );

    play.add_task(
        Task::new("Create volumes directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("docker/volumes")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    // Task 2: Create docker-compose file
    play.add_task(
        Task::new("Create docker-compose.yml", "copy")
            .arg(
                "content",
                r#"version: '3.8'

services:
  app:
    image: myapp:latest
    ports:
      - "8080:8080"
    environment:
      - DATABASE_URL=postgresql://postgres:password@db:5432/appdb
      - REDIS_URL=redis://redis:6379/0
      - APP_ENV=production
    depends_on:
      db:
        condition: service_healthy
      redis:
        condition: service_healthy
    networks:
      - app_network
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 10s
      retries: 3
    restart: unless-stopped

  db:
    image: postgres:15-alpine
    environment:
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
      - POSTGRES_DB=appdb
    volumes:
      - postgres_data:/var/lib/postgresql/data
    networks:
      - app_network
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U postgres"]
      interval: 10s
      timeout: 5s
      retries: 5
    restart: unless-stopped

  redis:
    image: redis:7-alpine
    command: redis-server --appendonly yes
    volumes:
      - redis_data:/data
    networks:
      - app_network
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5
    restart: unless-stopped

networks:
  app_network:
    driver: bridge

volumes:
  postgres_data:
  redis_data:
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("docker/docker-compose.yml")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 3: Create environment file
    play.add_task(
        Task::new("Create .env file", "copy")
            .arg(
                "content",
                r#"# Docker environment configuration
COMPOSE_PROJECT_NAME=myapp
APP_IMAGE=myapp:latest
POSTGRES_PASSWORD=secure_db_password
REDIS_PASSWORD=
APP_PORT=8080
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("docker/.env")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 4: Pull images (simulated)
    play.add_task(
        Task::new("Pull application image", "debug").arg("msg", "Pulling image: {{ app_image }}"),
    );

    play.add_task(
        Task::new("Pull Redis image", "debug").arg("msg", "Pulling image: {{ redis_image }}"),
    );

    play.add_task(
        Task::new("Pull PostgreSQL image", "debug")
            .arg("msg", "Pulling image: {{ postgres_image }}"),
    );

    // Task 5: Create network (simulated)
    play.add_task(
        Task::new("Create Docker network", "debug")
            .arg("msg", "Creating network: {{ network_name }}"),
    );

    // Task 6: Create volume directories
    play.add_task(
        Task::new("Create PostgreSQL data volume", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("docker/volumes/postgres")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    play.add_task(
        Task::new("Create Redis data volume", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("docker/volumes/redis")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    // Task 7: Create container startup script
    play.add_task(
        Task::new("Create startup script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Container startup script

cd /opt/docker

# Pull latest images
docker-compose pull

# Start containers in detached mode
docker-compose up -d

# Wait for health checks
echo "Waiting for services to be healthy..."
sleep 10

# Check status
docker-compose ps

echo "Container deployment complete"
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("docker/start.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 8: Create container health check script
    play.add_task(
        Task::new("Create health check script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Container health check script

# Check if all containers are running
running=$(docker-compose ps --filter "status=running" -q | wc -l)
expected=3

if [ "$running" -eq "$expected" ]; then
    echo "All containers running: $running/$expected"
else
    echo "Container count mismatch: $running/$expected"
    exit 1
fi

# Check app health endpoint
if curl -s http://localhost:8080/health > /dev/null; then
    echo "App health: OK"
else
    echo "App health: FAILED"
    exit 1
fi

echo "Health check passed"
exit 0
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("docker/healthcheck.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Task 9: Deploy containers (simulated)
    play.add_task(
        Task::new("Start containers", "debug")
            .arg("msg", "Starting containers with docker-compose"),
    );

    // Task 10: Verify deployment
    play.add_task(Task::new("Container deployment verification", "debug").arg(
        "msg",
        "Container deployment complete - app accessible at port 8080",
    ));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(
        !localhost_result.failed,
        "Container deployment should succeed"
    );

    // Verify deployment files
    assert!(temp_dir.path().join("docker/docker-compose.yml").exists());
    assert!(temp_dir.path().join("docker/.env").exists());
    assert!(temp_dir.path().join("docker/start.sh").exists());
    assert!(temp_dir.path().join("docker/healthcheck.sh").exists());
    assert!(temp_dir.path().join("docker/volumes/postgres").exists());
    assert!(temp_dir.path().join("docker/volumes/redis").exists());
}

// ============================================================================
// SCENARIO 8: Rolling Update
// ============================================================================
//
// This scenario simulates a rolling update across multiple hosts:
// - Serial deployment (one host at a time)
// - Health checks between updates
// - Rollback on failure
// - Zero-downtime deployment

#[tokio::test]
async fn test_scenario_rolling_update() {
    let executor = create_multi_host_executor();

    let mut playbook = Playbook::new("Rolling Update");

    playbook.set_var("app_version".to_string(), serde_json::json!("2.5.0"));
    playbook.set_var("rollback_version".to_string(), serde_json::json!("2.4.0"));
    playbook.set_var("health_check_retries".to_string(), serde_json::json!(3));
    playbook.set_var("health_check_delay".to_string(), serde_json::json!(5));

    // Play 1: Pre-update checks
    let mut pre_check_play = Play::new("Pre-update Checks", "webservers");
    pre_check_play.gather_facts = false;

    pre_check_play.add_task(
        Task::new("Check current version", "debug")
            .arg(
                "msg",
                "Checking current version on {{ inventory_hostname }}",
            )
            .register("current_version"),
    );

    pre_check_play.add_task(
        Task::new("Pre-update health check", "debug")
            .arg("msg", "Health check passed on {{ inventory_hostname }}"),
    );

    playbook.add_play(pre_check_play);

    // Play 2: Rolling update (would be serial in real scenario)
    let mut update_play = Play::new("Rolling Update to Web Servers", "webservers");
    update_play.gather_facts = false;

    // Remove from load balancer
    update_play.add_task(Task::new("Remove from load balancer", "debug").arg(
        "msg",
        "Removing {{ inventory_hostname }} from load balancer",
    ));

    // Wait for connections to drain
    update_play.add_task(Task::new("Drain connections", "pause").arg("seconds", 1));

    // Stop service
    update_play.add_task(
        Task::new("Stop application", "service")
            .arg("name", "myapp")
            .arg("state", "stopped"),
    );

    // Deploy new version
    update_play.add_task(Task::new("Deploy new version", "debug").arg(
        "msg",
        "Deploying version {{ app_version }} to {{ inventory_hostname }}",
    ));

    // Start service
    update_play.add_task(
        Task::new("Start application", "service")
            .arg("name", "myapp")
            .arg("state", "started"),
    );

    // Health check
    update_play.add_task(
        Task::new("Post-update health check", "debug")
            .arg("msg", "Running health check on {{ inventory_hostname }}")
            .register("health_check_result"),
    );

    // Add back to load balancer
    update_play.add_task(Task::new("Add to load balancer", "debug").arg(
        "msg",
        "Adding {{ inventory_hostname }} back to load balancer",
    ));

    update_play.add_task(
        Task::new("Update complete on host", "debug")
            .arg("msg", "Rolling update complete on {{ inventory_hostname }}"),
    );

    playbook.add_play(update_play);

    // Play 3: Post-update verification
    let mut verify_play = Play::new("Post-update Verification", "webservers");
    verify_play.gather_facts = false;

    verify_play.add_task(Task::new("Verify version", "debug").arg(
        "msg",
        "Verifying version {{ app_version }} on {{ inventory_hostname }}",
    ));

    verify_play.add_task(Task::new("Final health check", "debug").arg(
        "msg",
        "Final health check passed on {{ inventory_hostname }}",
    ));

    playbook.add_play(verify_play);

    // Play 4: Summary (all hosts)
    let mut summary_play = Play::new("Update Summary", "webservers");
    summary_play.gather_facts = false;

    summary_play.add_task(Task::new("Rolling update summary", "debug").arg(
        "msg",
        "Rolling update to version {{ app_version }} completed successfully",
    ));

    playbook.add_play(summary_play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify all web servers were updated
    for host in ["web1", "web2", "web3"] {
        let result = results.get(host).unwrap();
        assert!(!result.failed, "Rolling update should succeed on {}", host);
    }
}

// ============================================================================
// SCENARIO 9: Multi-Tier Application
// ============================================================================
//
// This scenario simulates deploying a complete multi-tier application:
// - Web tier: Load balancer + web servers
// - App tier: Application servers
// - Database tier: Database cluster
// - Configure inter-tier connectivity

#[tokio::test]
async fn test_scenario_multi_tier_application() {
    let executor = create_multi_host_executor();

    let mut playbook = Playbook::new("Multi-Tier Application Deployment");

    // Global variables
    playbook.set_var("environment".to_string(), serde_json::json!("production"));
    playbook.set_var("domain".to_string(), serde_json::json!("example.com"));

    // Play 1: Configure Load Balancer
    let mut lb_play = Play::new("Configure Load Balancer", "loadbalancers");
    lb_play.gather_facts = false;

    lb_play.set_var(
        "backend_servers".to_string(),
        serde_json::json!(["web1", "web2", "web3"]),
    );

    lb_play.add_task(
        Task::new("Install HAProxy", "package")
            .arg("name", "haproxy")
            .arg("state", "present"),
    );

    lb_play.add_task(
        Task::new("Configure HAProxy", "debug")
            .arg("msg", "Configuring HAProxy on {{ inventory_hostname }} with backends: {{ backend_servers }}")
            .notify("reload haproxy")
    );

    lb_play.add_task(
        Task::new("Start HAProxy", "service")
            .arg("name", "haproxy")
            .arg("state", "started")
            .arg("enabled", true),
    );

    lb_play.add_handler(Handler {
        name: "reload haproxy".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("haproxy"));
            args.insert("state".to_string(), serde_json::json!("reloaded"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(lb_play);

    // Play 2: Configure Web Tier
    let mut web_play = Play::new("Configure Web Tier", "webservers");
    web_play.gather_facts = false;

    web_play.set_var(
        "app_servers".to_string(),
        serde_json::json!(["app1", "app2"]),
    );

    web_play.add_task(
        Task::new("Install nginx", "package")
            .arg("name", "nginx")
            .arg("state", "present"),
    );

    web_play.add_task(
        Task::new("Configure nginx as reverse proxy", "debug")
            .arg(
                "msg",
                "Configuring nginx on {{ inventory_hostname }} to proxy to app servers",
            )
            .notify("reload nginx"),
    );

    web_play.add_task(
        Task::new("Start nginx", "service")
            .arg("name", "nginx")
            .arg("state", "started")
            .arg("enabled", true),
    );

    web_play.add_handler(Handler {
        name: "reload nginx".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("nginx"));
            args.insert("state".to_string(), serde_json::json!("reloaded"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(web_play);

    // Play 3: Configure Application Tier
    let mut app_play = Play::new("Configure Application Tier", "appservers");
    app_play.gather_facts = false;

    app_play.set_var("db_host".to_string(), serde_json::json!("db1"));
    app_play.set_var("db_port".to_string(), serde_json::json!(5432));

    app_play.add_task(
        Task::new("Install application dependencies", "package")
            .arg(
                "name",
                serde_json::json!(["python3", "python3-pip", "gunicorn"]),
            )
            .arg("state", "present"),
    );

    app_play.add_task(
        Task::new("Deploy application code", "debug")
            .arg("msg", "Deploying application on {{ inventory_hostname }}"),
    );

    app_play.add_task(Task::new("Configure database connection", "debug").arg(
        "msg",
        "Configuring connection to {{ db_host }}:{{ db_port }}",
    ));

    app_play.add_task(
        Task::new("Start application service", "service")
            .arg("name", "myapp")
            .arg("state", "started")
            .arg("enabled", true),
    );

    playbook.add_play(app_play);

    // Play 4: Configure Database Tier
    let mut db_play = Play::new("Configure Database Tier", "databases");
    db_play.gather_facts = false;

    db_play.set_var("replication_enabled".to_string(), serde_json::json!(true));

    db_play.add_task(
        Task::new("Install PostgreSQL", "package")
            .arg(
                "name",
                serde_json::json!(["postgresql", "postgresql-contrib"]),
            )
            .arg("state", "present"),
    );

    db_play.add_task(
        Task::new("Configure PostgreSQL", "debug")
            .arg("msg", "Configuring PostgreSQL on {{ inventory_hostname }}")
            .notify("restart postgresql"),
    );

    db_play.add_task(
        Task::new("Configure replication", "debug")
            .arg("msg", "Setting up replication on {{ inventory_hostname }}")
            .when("replication_enabled"),
    );

    db_play.add_task(
        Task::new("Start PostgreSQL", "service")
            .arg("name", "postgresql")
            .arg("state", "started")
            .arg("enabled", true),
    );

    db_play.add_handler(Handler {
        name: "restart postgresql".to_string(),
        module: "service".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("name".to_string(), serde_json::json!("postgresql"));
            args.insert("state".to_string(), serde_json::json!("restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(db_play);

    // Play 5: Configure Monitoring
    let mut mon_play = Play::new("Configure Monitoring", "monitoring");
    mon_play.gather_facts = false;

    mon_play.add_task(
        Task::new("Install Prometheus", "package")
            .arg("name", serde_json::json!(["prometheus", "grafana"]))
            .arg("state", "present"),
    );

    mon_play.add_task(
        Task::new("Configure Prometheus targets", "debug")
            .arg("msg", "Configuring monitoring targets for all tiers"),
    );

    mon_play.add_task(
        Task::new("Start monitoring services", "service")
            .arg("name", "prometheus")
            .arg("state", "started")
            .arg("enabled", true),
    );

    playbook.add_play(mon_play);

    // Play 6: Verify connectivity between tiers
    let mut verify_play = Play::new("Verify Inter-Tier Connectivity", "appservers");
    verify_play.gather_facts = false;

    verify_play.add_task(Task::new("Test database connectivity", "debug").arg(
        "msg",
        "Testing database connectivity from {{ inventory_hostname }}",
    ));

    verify_play.add_task(Task::new("Multi-tier deployment complete", "debug").arg(
        "msg",
        "All tiers configured and connected for {{ environment }}",
    ));

    playbook.add_play(verify_play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // Verify all tiers deployed successfully
    assert!(results.get("lb1").is_some());
    for host in ["web1", "web2", "web3"] {
        assert!(results.get(host).is_some());
    }
    for host in ["app1", "app2"] {
        assert!(results.get(host).is_some());
    }
    for host in ["db1", "db2"] {
        assert!(results.get(host).is_some());
    }
    assert!(results.get("mon1").is_some());

    // Verify no failures
    for (host, result) in &results {
        assert!(
            !result.failed,
            "Multi-tier deployment should succeed on {}",
            host
        );
    }
}

// ============================================================================
// SCENARIO 10: Disaster Recovery
// ============================================================================
//
// This scenario simulates disaster recovery procedures:
// - Backup configuration verification
// - Restore procedure testing
// - Failover testing
// - Recovery verification

#[tokio::test]
async fn test_scenario_disaster_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Disaster Recovery");

    playbook.set_var(
        "backup_server".to_string(),
        serde_json::json!("backup.example.com"),
    );
    playbook.set_var(
        "recovery_point".to_string(),
        serde_json::json!("2024-01-15-03:00"),
    );
    playbook.set_var(
        "services".to_string(),
        serde_json::json!(["postgresql", "myapp", "nginx"]),
    );

    // Play 1: Backup Verification
    let mut backup_play = Play::new("Verify Backup Configuration", "localhost");
    backup_play.gather_facts = false;

    // Create backup directory structure
    backup_play.add_task(
        Task::new("Create backup directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("backups")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    backup_play.add_task(
        Task::new("Create backup config directory", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("backups/config")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    // Create backup configuration
    backup_play.add_task(
        Task::new("Create backup configuration", "copy")
            .arg(
                "content",
                r#"# Backup Configuration
BACKUP_SERVER=backup.example.com
BACKUP_USER=backup
BACKUP_PATH=/backups
RETENTION_DAYS=30

# Database backup settings
DB_BACKUP_ENABLED=true
DB_HOST=localhost
DB_NAME=appdb
DB_USER=postgres

# Application backup settings
APP_BACKUP_ENABLED=true
APP_PATH=/opt/apps/current
APP_EXCLUDE=".git,node_modules,__pycache__"

# Config backup settings
CONFIG_BACKUP_ENABLED=true
CONFIG_PATHS="/etc/nginx,/etc/postgresql,/etc/myapp"
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("backups/config/backup.conf")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Create backup script
    backup_play.add_task(
        Task::new("Create backup script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Full Backup Script

DATE=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/backups/$DATE"

echo "Starting full backup: $DATE"
mkdir -p "$BACKUP_DIR"

# Database backup
echo "Backing up database..."
pg_dump -U postgres appdb | gzip > "$BACKUP_DIR/database.sql.gz"

# Application backup
echo "Backing up application..."
tar -czf "$BACKUP_DIR/application.tar.gz" -C /opt/apps current --exclude='.git'

# Configuration backup
echo "Backing up configuration..."
tar -czf "$BACKUP_DIR/config.tar.gz" /etc/nginx /etc/postgresql /etc/myapp

# Create manifest
echo "Creating backup manifest..."
cat > "$BACKUP_DIR/manifest.json" <<EOF
{
    "date": "$DATE",
    "database": "database.sql.gz",
    "application": "application.tar.gz",
    "config": "config.tar.gz",
    "checksums": "$(md5sum $BACKUP_DIR/*.gz)"
}
EOF

echo "Backup complete: $BACKUP_DIR"
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("backups/backup.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Verify backup configuration
    backup_play.add_task(Task::new("Verify backup configuration", "debug").arg(
        "msg",
        "Backup configuration verified - target: {{ backup_server }}",
    ));

    playbook.add_play(backup_play);

    // Play 2: Restore Procedure
    let mut restore_play = Play::new("Test Restore Procedure", "localhost");
    restore_play.gather_facts = false;

    // Create restore script
    restore_play.add_task(
        Task::new("Create restore script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Disaster Recovery Restore Script

BACKUP_DATE="${1:-latest}"
BACKUP_DIR="/backups/$BACKUP_DATE"

echo "=== Disaster Recovery Restore ==="
echo "Restoring from: $BACKUP_DIR"

# Stop services
echo "Stopping services..."
systemctl stop myapp nginx

# Restore database
echo "Restoring database..."
gunzip -c "$BACKUP_DIR/database.sql.gz" | psql -U postgres appdb

# Restore application
echo "Restoring application..."
rm -rf /opt/apps/current
tar -xzf "$BACKUP_DIR/application.tar.gz" -C /opt/apps

# Restore configuration
echo "Restoring configuration..."
tar -xzf "$BACKUP_DIR/config.tar.gz" -C /

# Start services
echo "Starting services..."
systemctl start postgresql nginx myapp

# Verify services
echo "Verifying services..."
for svc in postgresql nginx myapp; do
    if systemctl is-active --quiet $svc; then
        echo "$svc: OK"
    else
        echo "$svc: FAILED"
        exit 1
    fi
done

echo "Restore complete!"
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("backups/restore.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    restore_play.add_task(Task::new("Simulate restore verification", "debug").arg(
        "msg",
        "Testing restore from recovery point: {{ recovery_point }}",
    ));

    playbook.add_play(restore_play);

    // Play 3: Failover Testing
    let mut failover_play = Play::new("Failover Testing", "localhost");
    failover_play.gather_facts = false;

    // Create failover configuration
    failover_play.add_task(
        Task::new("Create failover configuration", "copy")
            .arg(
                "content",
                r#"# Failover Configuration

# Primary database
PRIMARY_DB_HOST=db1.example.com
PRIMARY_DB_PORT=5432

# Secondary database
SECONDARY_DB_HOST=db2.example.com
SECONDARY_DB_PORT=5432

# Failover settings
FAILOVER_TIMEOUT=30
HEALTH_CHECK_INTERVAL=5
MAX_REPLICATION_LAG=100

# DNS failover
DNS_ZONE=example.com
DNS_RECORD=db
DNS_TTL=60
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("backups/config/failover.conf")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    // Create failover script
    failover_play.add_task(
        Task::new("Create failover script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Automated Failover Script

source /etc/dr/failover.conf

echo "=== Failover Procedure ==="

# Check primary health
check_primary() {
    pg_isready -h $PRIMARY_DB_HOST -p $PRIMARY_DB_PORT -t 5 > /dev/null 2>&1
    return $?
}

# Check secondary health
check_secondary() {
    pg_isready -h $SECONDARY_DB_HOST -p $SECONDARY_DB_PORT -t 5 > /dev/null 2>&1
    return $?
}

# Perform failover
do_failover() {
    echo "Initiating failover to secondary..."

    # Promote secondary to primary
    ssh $SECONDARY_DB_HOST "pg_ctl promote -D /var/lib/postgresql/data"

    # Update DNS
    echo "Updating DNS records..."
    # nsupdate commands would go here

    # Update application configs
    echo "Updating application configurations..."
    sed -i "s/$PRIMARY_DB_HOST/$SECONDARY_DB_HOST/g" /etc/myapp/database.conf

    # Restart applications
    systemctl restart myapp

    echo "Failover complete!"
}

# Main logic
if ! check_primary; then
    echo "Primary is DOWN"
    if check_secondary; then
        echo "Secondary is UP - initiating failover"
        do_failover
    else
        echo "CRITICAL: Both primary and secondary are down!"
        exit 1
    fi
else
    echo "Primary is healthy - no failover needed"
fi
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("backups/failover.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    failover_play.add_task(Task::new("Test failover scenario", "debug").arg(
        "msg",
        "Failover testing completed - secondary ready for promotion",
    ));

    playbook.add_play(failover_play);

    // Play 4: Recovery Verification
    let mut verify_play = Play::new("Recovery Verification", "localhost");
    verify_play.gather_facts = false;

    // Create recovery test script
    verify_play.add_task(
        Task::new("Create recovery test script", "copy")
            .arg(
                "content",
                r#"#!/bin/bash
# Recovery Verification Script

echo "=== Recovery Verification ==="

# Check all services
echo "Checking services..."
for svc in postgresql nginx myapp; do
    if systemctl is-active --quiet $svc; then
        echo "  $svc: UP"
    else
        echo "  $svc: DOWN"
        FAILED=1
    fi
done

# Check database connectivity
echo "Checking database..."
if psql -U postgres -d appdb -c "SELECT 1" > /dev/null 2>&1; then
    echo "  Database: OK"
else
    echo "  Database: FAILED"
    FAILED=1
fi

# Check application health
echo "Checking application..."
if curl -s http://localhost:8080/health | grep -q "healthy"; then
    echo "  Application: OK"
else
    echo "  Application: FAILED"
    FAILED=1
fi

# Check data integrity
echo "Checking data integrity..."
ROW_COUNT=$(psql -U postgres -d appdb -t -c "SELECT COUNT(*) FROM users")
if [ "$ROW_COUNT" -gt 0 ]; then
    echo "  Data integrity: OK ($ROW_COUNT records)"
else
    echo "  Data integrity: WARNING (no records)"
fi

if [ -n "$FAILED" ]; then
    echo ""
    echo "RECOVERY VERIFICATION: FAILED"
    exit 1
else
    echo ""
    echo "RECOVERY VERIFICATION: PASSED"
    exit 0
fi
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("backups/verify_recovery.sh")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    verify_play.add_task(
        Task::new("Run recovery verification", "debug")
            .arg("msg", "Running recovery verification checks"),
    );

    verify_play.add_task(Task::new("Disaster recovery test complete", "debug").arg(
        "msg",
        "DR procedure verified - backup from {{ recovery_point }} recoverable",
    ));

    playbook.add_play(verify_play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed, "Disaster recovery should succeed");

    // Verify DR files created
    assert!(temp_dir.path().join("backups/config/backup.conf").exists());
    assert!(temp_dir.path().join("backups/backup.sh").exists());
    assert!(temp_dir.path().join("backups/restore.sh").exists());
    assert!(temp_dir
        .path()
        .join("backups/config/failover.conf")
        .exists());
    assert!(temp_dir.path().join("backups/failover.sh").exists());
    assert!(temp_dir.path().join("backups/verify_recovery.sh").exists());
}

// ============================================================================
// Comprehensive End-to-End Test
// ============================================================================
//
// This test combines multiple scenarios to simulate a real-world deployment

#[tokio::test]
async fn test_comprehensive_deployment_pipeline() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Comprehensive Deployment Pipeline");

    // Global configuration
    playbook.set_var("environment".to_string(), serde_json::json!("staging"));
    playbook.set_var("app_name".to_string(), serde_json::json!("myapp"));
    playbook.set_var("version".to_string(), serde_json::json!("1.0.0"));

    let mut play = Play::new("Full Stack Deployment", "localhost");
    play.gather_facts = false;

    // Phase 1: Infrastructure Setup
    play.add_task(
        Task::new("Create infrastructure directories", "file")
            .arg(
                "path",
                temp_dir
                    .path()
                    .join("infrastructure")
                    .to_string_lossy()
                    .to_string(),
            )
            .arg("state", "directory"),
    );

    // Phase 2: Security Configuration
    play.add_task(
        Task::new("Apply security baseline", "debug")
            .arg("msg", "Applying security baseline for {{ environment }}"),
    );

    // Phase 3: Service Installation
    play.add_task(
        Task::new("Install required services", "package")
            .arg("name", serde_json::json!(["nginx", "postgresql", "redis"]))
            .arg("state", "present"),
    );

    // Phase 4: Application Deployment
    play.add_task(
        Task::new("Deploy application", "debug")
            .arg("msg", "Deploying {{ app_name }} version {{ version }}"),
    );

    // Phase 5: Configuration
    play.add_task(Task::new("Configure services", "debug").arg(
        "msg",
        "Configuring services for {{ environment }} environment",
    ));

    // Phase 6: Start Services
    play.add_task(Task::new("Start all services", "debug").arg(
        "msg",
        "Starting services: nginx, postgresql, redis, {{ app_name }}",
    ));

    // Phase 7: Monitoring Setup
    play.add_task(
        Task::new("Configure monitoring", "debug")
            .arg("msg", "Setting up monitoring for {{ app_name }}"),
    );

    // Phase 8: Health Verification
    play.add_task(Task::new("Verify deployment health", "debug").arg(
        "msg",
        "All health checks passed for {{ app_name }} v{{ version }}",
    ));

    // Phase 9: Documentation
    play.add_task(
        Task::new("Generate deployment report", "copy")
            .arg(
                "content",
                r#"# Deployment Report
Environment: staging
Application: myapp
Version: 1.0.0
Status: SUCCESS

## Services Deployed
- nginx
- postgresql
- redis
- myapp

## Health Check Results
- All services: HEALTHY
- Database connectivity: OK
- Application endpoints: OK

## Next Steps
1. Monitor for 24 hours
2. Run integration tests
3. Schedule production deployment
"#,
            )
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("infrastructure/deployment_report.md")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(
        !localhost_result.failed,
        "Comprehensive deployment should succeed"
    );
    assert!(localhost_result.stats.changed > 0 || localhost_result.stats.ok > 0);

    // Verify final artifacts
    assert!(temp_dir.path().join("infrastructure").exists());
    assert!(temp_dir
        .path()
        .join("infrastructure/deployment_report.md")
        .exists());
}

// ============================================================================
// Execution Strategy Tests
// ============================================================================

#[tokio::test]
async fn test_free_strategy_parallel_execution() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("host1".to_string(), Some("all"));
    runtime.add_host("host2".to_string(), Some("all"));
    runtime.add_host("host3".to_string(), Some("all"));

    let config = ExecutorConfig {
        forks: 10,
        strategy: ExecutionStrategy::Free,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Free Strategy Test");
    let mut play = Play::new("Parallel Tasks", "all");
    play.gather_facts = false;

    // These tasks should run independently on each host
    play.add_task(Task::new("Task 1", "debug").arg("msg", "Task 1 on {{ inventory_hostname }}"));

    play.add_task(Task::new("Task 2", "debug").arg("msg", "Task 2 on {{ inventory_hostname }}"));

    play.add_task(Task::new("Task 3", "debug").arg("msg", "Task 3 on {{ inventory_hostname }}"));

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    // All hosts should have completed successfully
    assert_eq!(results.len(), 3);
    for result in results.values() {
        assert!(!result.failed);
    }
}

#[tokio::test]
async fn test_error_handling_with_ignore_errors() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Error Handling Test");
    let mut play = Play::new("Test ignore_errors", "localhost");
    play.gather_facts = false;

    // Task that will fail but should be ignored
    play.add_task(
        Task::new("Failing task (ignored)", "copy")
            .arg("src", "/nonexistent/path/that/does/not/exist")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("target.txt")
                    .to_string_lossy()
                    .to_string(),
            )
            .ignore_errors(true),
    );

    // This task should still run
    play.add_task(
        Task::new("Following task", "copy")
            .arg("content", "Success after error")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("success.txt")
                    .to_string_lossy()
                    .to_string(),
            ),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let _localhost_result = results.get("localhost").unwrap();
    // The following task should have created the file
    assert!(temp_dir.path().join("success.txt").exists());
}

#[tokio::test]
async fn test_conditional_execution_with_when() {
    let temp_dir = TempDir::new().unwrap();

    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);
    runtime.set_host_fact(
        "localhost",
        "ansible_os_family".to_string(),
        serde_json::json!("Debian"),
    );

    let config = ExecutorConfig {
        forks: 1,
        gather_facts: false,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Conditional Test");
    let mut play = Play::new("Test Conditions", "localhost");
    play.gather_facts = false;

    play.set_var("install_nginx".to_string(), serde_json::json!(true));
    play.set_var("install_apache".to_string(), serde_json::json!(false));

    // Should run (condition true)
    play.add_task(
        Task::new("Install nginx (should run)", "copy")
            .arg("content", "nginx installed")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("nginx.txt")
                    .to_string_lossy()
                    .to_string(),
            )
            .when("install_nginx"),
    );

    // Should skip (condition false)
    play.add_task(
        Task::new("Install apache (should skip)", "copy")
            .arg("content", "apache installed")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("apache.txt")
                    .to_string_lossy()
                    .to_string(),
            )
            .when("install_apache"),
    );

    // Should run (fact-based condition)
    play.add_task(
        Task::new("Debian-specific task", "copy")
            .arg("content", "Debian configured")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("debian.txt")
                    .to_string_lossy()
                    .to_string(),
            )
            .when("ansible_os_family == 'Debian'"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed);

    // Verify conditional execution
    assert!(
        temp_dir.path().join("nginx.txt").exists(),
        "nginx should be installed"
    );
    assert!(
        !temp_dir.path().join("apache.txt").exists(),
        "apache should be skipped"
    );
    assert!(
        temp_dir.path().join("debian.txt").exists(),
        "debian task should run"
    );
}

#[tokio::test]
async fn test_handler_notification_chain() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Handler Chain Test");
    let mut play = Play::new("Test Handler Chain", "localhost");
    play.gather_facts = false;

    // Task that notifies handler
    play.add_task(
        Task::new("Update config", "copy")
            .arg("content", "new configuration")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("config.txt")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("restart service"),
    );

    // Another task that notifies the same handler
    play.add_task(
        Task::new("Update another config", "copy")
            .arg("content", "another configuration")
            .arg(
                "dest",
                temp_dir
                    .path()
                    .join("config2.txt")
                    .to_string_lossy()
                    .to_string(),
            )
            .notify("restart service"),
    );

    // Handler should only run once even if notified multiple times
    play.add_handler(Handler {
        name: "restart service".to_string(),
        module: "debug".to_string(),
        args: {
            let mut args = indexmap::IndexMap::new();
            args.insert("msg".to_string(), serde_json::json!("Service restarted"));
            args
        },
        when: None,
        listen: vec![],
    });

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed);
    assert!(temp_dir.path().join("config.txt").exists());
    assert!(temp_dir.path().join("config2.txt").exists());
}

#[tokio::test]
async fn test_loop_execution() {
    let temp_dir = TempDir::new().unwrap();
    let executor = create_local_executor(&temp_dir);

    let mut playbook = Playbook::new("Loop Test");
    let mut play = Play::new("Test Loops", "localhost");
    play.gather_facts = false;

    // Create multiple files in a loop
    play.add_task(
        Task::new("Create files", "debug")
            .arg("msg", "Processing {{ item }}")
            .loop_over(vec![
                serde_json::json!("file1"),
                serde_json::json!("file2"),
                serde_json::json!("file3"),
            ]),
    );

    // Loop with object items
    play.add_task(
        Task::new("Configure services", "debug")
            .arg("msg", "Configuring {{ item.name }} on port {{ item.port }}")
            .loop_over(vec![
                serde_json::json!({"name": "web", "port": 80}),
                serde_json::json!({"name": "api", "port": 8080}),
                serde_json::json!({"name": "admin", "port": 9000}),
            ]),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed);
}

#[tokio::test]
async fn test_variable_precedence() {
    let mut runtime = RuntimeContext::new();
    runtime.add_host("localhost".to_string(), None);

    // Set global var (lowest priority)
    runtime.set_global_var("priority_test".to_string(), serde_json::json!("global"));

    // Set host var
    runtime.set_host_var(
        "localhost",
        "priority_test".to_string(),
        serde_json::json!("host"),
    );

    // Extra vars (highest priority)
    let mut extra_vars = HashMap::new();
    extra_vars.insert("priority_test".to_string(), serde_json::json!("extra"));

    let config = ExecutorConfig {
        forks: 1,
        gather_facts: false,
        extra_vars,
        ..Default::default()
    };

    let executor = Executor::with_runtime(config, runtime);

    let mut playbook = Playbook::new("Variable Precedence Test");

    // Playbook var
    playbook.set_var("priority_test".to_string(), serde_json::json!("playbook"));

    let mut play = Play::new("Test Variables", "localhost");
    play.gather_facts = false;

    // Play var
    play.set_var("priority_test".to_string(), serde_json::json!("play"));

    play.add_task(
        Task::new("Show variable", "debug").arg("msg", "priority_test = {{ priority_test }}"),
    );

    playbook.add_play(play);

    let results = executor.run_playbook(&playbook).await.unwrap();

    let localhost_result = results.get("localhost").unwrap();
    assert!(!localhost_result.failed);
    // Extra vars should win due to highest precedence
}
