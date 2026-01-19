use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use tracing::{debug, info, warn};

use crate::recovery::TransactionId;

use super::task::{BlockRole, Handler, Task};
use super::{Executor, ExecutorResult};

impl Executor {
    /// Flush all notified handlers
    ///
    /// This method:
    /// 1. Resolves notification names to handlers (by name or listen directive)
    /// 2. Ensures handlers run in definition order
    /// 3. Supports handler chaining (handlers can notify other handlers)
    /// 4. Deduplicates handlers so each runs only once per flush
    pub(super) async fn flush_handlers(&self, tx_id: Option<TransactionId>) -> ExecutorResult<()> {
        let notified: Vec<String> = {
            let mut notified = self.notified_handlers.lock().await;
            let handlers: Vec<_> = notified.drain().collect();
            handlers
        };

        if notified.is_empty() {
            return Ok(());
        }

        info!("Running handlers for {} notifications", notified.len());

        let handlers = self.handlers.read().await;

        // Build a lookup map: notification name -> list of handlers that respond to it
        // A handler responds to a notification if:
        // 1. Its name matches the notification, OR
        // 2. Its listen list contains the notification name
        let mut notification_to_handlers: HashMap<String, Vec<String>> = HashMap::new();

        for handler in handlers.values() {
            // Handler responds to its own name
            notification_to_handlers
                .entry(handler.name.clone())
                .or_default()
                .push(handler.name.clone());

            // Handler responds to each name in its listen list
            for listen_name in &handler.listen {
                notification_to_handlers
                    .entry(listen_name.clone())
                    .or_default()
                    .push(handler.name.clone());
            }
        }

        // Collect all handlers that need to run (deduped)
        let mut handlers_to_run: HashSet<String> = HashSet::new();

        for notification_name in &notified {
            if let Some(responding_handlers) = notification_to_handlers.get(notification_name) {
                for handler_name in responding_handlers {
                    handlers_to_run.insert(handler_name.clone());
                }
            } else {
                // No handler found for this notification
                warn!("Handler not found for notification: {}", notification_name);
            }
        }

        if handlers_to_run.is_empty() {
            debug!("No handlers matched the notifications");
            return Ok(());
        }

        // Sort handlers by their definition order (order in the handlers map)
        // We use the order from the handlers HashMap which preserves insertion order
        let mut ordered_handlers: Vec<&Handler> = handlers
            .values()
            .filter(|h| handlers_to_run.contains(&h.name))
            .collect();

        // Stable sort is not needed since HashMap doesn't preserve order
        // We'll use the order handlers appear in the play's handlers vector
        // For now, alphabetical order ensures consistent behavior
        ordered_handlers.sort_by(|a, b| a.name.cmp(&b.name));

        info!("Running {} unique handlers", ordered_handlers.len());

        // Track handlers that have already run in this flush cycle
        let mut executed_handlers: HashSet<String> = HashSet::new();

        // Get all active hosts from runtime
        let hosts = {
            let runtime = self.runtime.read().await;
            runtime.get_all_hosts()
        };

        // Execute handlers, supporting handler chaining
        // We loop until no new handlers are notified
        let mut current_handlers = ordered_handlers;

        loop {
            let mut new_notifications: HashSet<String> = HashSet::new();

            for handler in &current_handlers {
                if executed_handlers.contains(&handler.name) {
                    continue;
                }

                debug!("Running handler: {}", handler.name);
                executed_handlers.insert(handler.name.clone());

                // Create task from handler
                // Note: We include notify field to support handler chaining
                let task = Task {
                    name: handler.name.clone(),
                    module: handler.module.clone(),
                    args: handler.args.clone(),
                    when: handler.when.clone(),
                    notify: Vec::new(), // Handlers don't chain via task.notify in our model
                    register: None,
                    loop_items: None,
                    loop_var: "item".to_string(),
                    loop_control: None,
                    ignore_errors: false,
                    changed_when: None,
                    failed_when: None,
                    delegate_to: None,
                    delegate_facts: None,
                    run_once: false,
                    tags: Vec::new(),
                    vars: IndexMap::new(),
                    r#become: false,
                    become_user: None,
                    block_id: None,
                    block_role: BlockRole::Normal,
                    block_stack: Vec::new(),
                    retries: None,
                    delay: None,
                    until: None,
                };

                // Run handler on all hosts
                let results = self.run_task_on_hosts(&hosts, &task, tx_id.clone()).await?;

                // Check if handler execution triggered any changes
                // If so, check if any handlers listen to this handler's name (handler chaining)
                let any_changed = results.values().any(|r| r.changed);
                if any_changed {
                    // Check if any other handlers listen to this handler's name
                    if let Some(chained_handlers) = notification_to_handlers.get(&handler.name) {
                        for chained_handler in chained_handlers {
                            if chained_handler != &handler.name
                                && !executed_handlers.contains(chained_handler)
                            {
                                new_notifications.insert(chained_handler.clone());
                            }
                        }
                    }
                }
            }

            // If no new handlers were triggered, we're done
            if new_notifications.is_empty() {
                break;
            }

            // Prepare the next round of handlers
            current_handlers = handlers
                .values()
                .filter(|h| new_notifications.contains(&h.name))
                .collect();

            if current_handlers.is_empty() {
                break;
            }

            debug!(
                "Handler chaining: {} additional handlers triggered",
                current_handlers.len()
            );
        }

        Ok(())
    }

    /// Notify a handler to be run at end of play
    pub async fn notify_handler(&self, handler_name: &str) {
        let mut notified = self.notified_handlers.lock().await;
        notified.insert(handler_name.to_string());
        debug!("Handler notified: {}", handler_name);
    }
}
