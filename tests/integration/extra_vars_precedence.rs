//! Integration tests for extra vars precedence (Issue #51)
//!
//! Verifies that command-line extra vars correctly override all other variable sources
//! including playbook vars, play vars, and global vars.

use rustible::executor::{Executor, ExecutorConfig};
use rustible::executor::playbook::{Play, Playbook};
use serde_json::json;
use std::collections::HashMap;

#[tokio::test]
async fn test_extra_vars_override_playbook_vars() {
    // Create a playbook with a variable
    let playbook = Playbook {
        name: "Test Playbook".to_string(),
        vars: {
            let mut vars = HashMap::new();
            vars.insert("test_var".to_string(), json!("playbook_value"));
            vars
        },
        plays: vec![],
    };

    // Create executor with extra vars
    let mut config = ExecutorConfig::default();
    config.extra_vars.insert("test_var".to_string(), json!("extra_value"));

    let executor = Executor::new(config);

    // Initialize runtime with playbook vars
    let runtime = executor.runtime();
    {
        let mut rt = runtime.write().await;
        for (key, value) in &playbook.vars {
            rt.set_global_var(key.clone(), value.clone());
        }
        // Extra vars should override
        for (key, value) in &executor.config.extra_vars {
            rt.set_extra_var(key.clone(), value.clone());
        }
    }

    // Verify extra vars have highest precedence
    {
        let rt = runtime.read().await;
        let value = rt.get_var("test_var", None).unwrap();
        assert_eq!(value, json!("extra_value"), "Extra vars should override playbook vars");
    }
}

#[tokio::test]
async fn test_extra_vars_override_play_vars() {
    let mut config = ExecutorConfig::default();
    config.extra_vars.insert("test_var".to_string(), json!("extra_value"));

    let executor = Executor::new(config);
    let runtime = executor.runtime();

    {
        let mut rt = runtime.write().await;
        rt.set_play_var("test_var".to_string(), json!("play_value"));
        rt.set_extra_var("test_var".to_string(), json!("extra_value"));
    }

    {
        let rt = runtime.read().await;
        let value = rt.get_var("test_var", None).unwrap();
        assert_eq!(value, json!("extra_value"), "Extra vars should override play vars");
    }
}

#[tokio::test]
async fn test_extra_vars_override_global_vars() {
    let mut config = ExecutorConfig::default();
    config.extra_vars.insert("test_var".to_string(), json!("extra_value"));

    let executor = Executor::new(config);
    let runtime = executor.runtime();

    {
        let mut rt = runtime.write().await;
        rt.set_global_var("test_var".to_string(), json!("global_value"));
        rt.set_extra_var("test_var".to_string(), json!("extra_value"));
    }

    {
        let rt = runtime.read().await;
        let value = rt.get_var("test_var", None).unwrap();
        assert_eq!(value, json!("extra_value"), "Extra vars should override global vars");
    }
}

#[tokio::test]
async fn test_variable_precedence_order() {
    // Test complete precedence: extra > task > play > global
    let executor = Executor::new(ExecutorConfig::default());
    let runtime = executor.runtime();

    {
        let mut rt = runtime.write().await;
        rt.set_global_var("var1".to_string(), json!("global"));
        rt.set_play_var("var1".to_string(), json!("play"));
        rt.set_task_var("var1".to_string(), json!("task"));
        rt.set_extra_var("var1".to_string(), json!("extra"));
    }

    {
        let rt = runtime.read().await;
        assert_eq!(rt.get_var("var1", None).unwrap(), json!("extra"));
    }

    // Remove extra, task should win
    {
        let mut rt = runtime.write().await;
        rt.extra_vars.shift_remove("var1");
    }

    {
        let rt = runtime.read().await;
        assert_eq!(rt.get_var("var1", None).unwrap(), json!("task"));
    }

    // Remove task, play should win
    {
        let mut rt = runtime.write().await;
        rt.clear_task_vars();
    }

    {
        let rt = runtime.read().await;
        assert_eq!(rt.get_var("var1", None).unwrap(), json!("play"));
    }

    // Remove play, global should win
    {
        let mut rt = runtime.write().await;
        rt.clear_play_vars();
    }

    {
        let rt = runtime.read().await;
        assert_eq!(rt.get_var("var1", None).unwrap(), json!("global"));
    }
}
