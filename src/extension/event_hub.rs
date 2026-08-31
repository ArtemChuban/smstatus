use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use super::bus::{ExtensionEvent, ExtensionEventBus};

const MAX_QUEUE_LEN: usize = 64;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn push_capped(queue: &mut VecDeque<ExtensionEvent>, event: ExtensionEvent) {
    while queue.len() >= MAX_QUEUE_LEN {
        queue.pop_front();
    }
    queue.push_back(event);
}

pub(crate) struct ExtensionEventHub {
    interests: Mutex<HashMap<String, HashMap<String, HashSet<String>>>>,
    queues: Mutex<HashMap<String, VecDeque<ExtensionEvent>>>,
}

impl ExtensionEventHub {
    pub(crate) fn new() -> Self {
        Self {
            interests: Mutex::new(HashMap::new()),
            queues: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn subscribe(&self, instance: &str, extension: &str, event: &str) {
        lock(&self.interests)
            .entry(extension.to_string())
            .or_default()
            .entry(event.to_string())
            .or_default()
            .insert(instance.to_string());
    }

    pub(crate) fn deliver(&self, extension: &str, event: &str, payload: &str) -> Vec<String> {
        let subscribers: Vec<String> = {
            let interests = lock(&self.interests);
            interests
                .get(extension)
                .and_then(|events| events.get(event))
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default()
        };
        if subscribers.is_empty() {
            return Vec::new();
        }
        let mut queues = lock(&self.queues);
        for instance in &subscribers {
            let queue = queues.entry(instance.clone()).or_default();
            push_capped(
                queue,
                ExtensionEvent {
                    extension: extension.to_string(),
                    event: event.to_string(),
                    payload: payload.to_string(),
                },
            );
        }
        subscribers
    }

    pub(crate) fn take(&self, instance: &str) -> Option<ExtensionEvent> {
        lock(&self.queues)
            .get_mut(instance)
            .and_then(|queue| queue.pop_front())
    }

    pub(crate) fn retain_instances(&self, keep: &HashSet<&str>) {
        {
            let mut interests = lock(&self.interests);
            for events in interests.values_mut() {
                for subscribers in events.values_mut() {
                    subscribers.retain(|name| keep.contains(name.as_str()));
                }
                events.retain(|_, subscribers| !subscribers.is_empty());
            }
            interests.retain(|_, events| !events.is_empty());
        }
        lock(&self.queues).retain(|name, _| keep.contains(name.as_str()));
    }

    pub(crate) fn clear_all(&self) {
        lock(&self.interests).clear();
        lock(&self.queues).clear();
    }

    pub(crate) fn spawn_bus_pump(
        self: &Arc<Self>,
        bus: &ExtensionEventBus,
        wake_tx: Option<std::sync::mpsc::Sender<Vec<String>>>,
    ) {
        let hub = Arc::clone(self);
        let rx: Receiver<ExtensionEvent> = bus.subscribe();
        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let names = hub.deliver(&event.extension, &event.event, &event.payload);
                if !names.is_empty()
                    && let Some(tx) = &wake_tx
                {
                    let _ = tx.send(names);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::ExtensionEventBus;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn deliver_notifies_wake_sender_with_subscriber_names() {
        let bus = Arc::new(ExtensionEventBus::new());
        let hub = Arc::new(ExtensionEventHub::new());
        let (wake_tx, wake_rx) = mpsc::channel();
        hub.subscribe("keyboard", "xkb", "state-changed");
        hub.spawn_bus_pump(bus.as_ref(), Some(wake_tx));

        bus.publish_parts("xkb", "state-changed", "{}");
        let names = wake_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(names, vec!["keyboard".to_string()]);
        assert!(hub.take("keyboard").is_some());
    }

    #[test]
    fn clear_all_stops_delivery_and_drops_queues() {
        let hub = ExtensionEventHub::new();
        hub.subscribe("keyboard", "xkb", "state-changed");
        hub.deliver("xkb", "state-changed", "1");
        assert!(hub.take("keyboard").is_some());

        hub.subscribe("keyboard", "xkb", "state-changed");
        hub.deliver("xkb", "state-changed", "2");
        hub.clear_all();
        assert!(hub.take("keyboard").is_none());
        assert!(hub.deliver("xkb", "state-changed", "3").is_empty());
    }

    #[test]
    fn retain_instances_drops_others() {
        let hub = ExtensionEventHub::new();
        hub.subscribe("a", "xkb", "state-changed");
        hub.subscribe("b", "xkb", "state-changed");
        let keep: HashSet<&str> = HashSet::from(["a"]);
        hub.retain_instances(&keep);
        let names = hub.deliver("xkb", "state-changed", "{}");
        assert_eq!(names, vec!["a".to_string()]);
    }

    #[test]
    fn deliver_caps_queue_length_dropping_oldest() {
        let hub = ExtensionEventHub::new();
        hub.subscribe("keyboard", "xkb", "state-changed");
        for i in 0..(MAX_QUEUE_LEN + 10) {
            hub.deliver("xkb", "state-changed", &i.to_string());
        }
        let first = hub.take("keyboard").unwrap();
        assert_eq!(first.payload, "10");
        let mut count = 1;
        while hub.take("keyboard").is_some() {
            count += 1;
        }
        assert_eq!(count, MAX_QUEUE_LEN);
    }
}
