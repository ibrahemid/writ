use crate::watcher::change_event::ExternalChange;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WritEvent {
    ConfigChanged {
        keys: Vec<String>,
    },
    BufferExternal {
        buffer_id: String,
        change: ExternalChange,
    },
    RecoveryDirty {
        snapshot_id: String,
        buffer_count: u32,
    },
    HotkeyToggle,
    PluginEvent {
        plugin_id: String,
        data: serde_json::Value,
    },
}

type Subscriber = Arc<dyn Fn(&WritEvent) + Send + Sync>;

pub struct EventBus {
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn subscribe<F>(&self, handler: F)
    where
        F: Fn(&WritEvent) + Send + Sync + 'static,
    {
        self.subscribers
            .lock()
            .expect("subscribers lock poisoned")
            .push(Arc::new(handler));
    }

    pub fn emit(&self, event: WritEvent) {
        let subscribers = self
            .subscribers
            .lock()
            .expect("subscribers lock poisoned")
            .clone();
        for subscriber in subscribers {
            subscriber(&event);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
