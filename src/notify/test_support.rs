use std::env;
use std::sync::Mutex;

pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct EnvGuard {
    saved: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    pub(crate) fn new() -> Self {
        Self { saved: Vec::new() }
    }

    pub(crate) fn set(&mut self, key: &str, value: &str) {
        let prev = env::var(key).ok();
        self.saved.push((key.to_string(), prev));
        env::set_var(key, value);
    }

    pub(crate) fn remove(&mut self, key: &str) {
        let prev = env::var(key).ok();
        self.saved.push((key.to_string(), prev));
        env::remove_var(key);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            if let Some(val) = value {
                env::set_var(key, val);
            } else {
                env::remove_var(key);
            }
        }
    }
}
