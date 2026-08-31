use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, MutexGuard, PoisonError};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionEvent {
    pub extension: String,
    pub event: String,
    pub payload: String,
}

pub(crate) struct ExtensionEventBus {
    listeners: Mutex<Vec<Sender<ExtensionEvent>>>,
}

impl ExtensionEventBus {
    pub(crate) fn new() -> Self {
        Self {
            listeners: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn subscribe(&self) -> Receiver<ExtensionEvent> {
        let (tx, rx) = mpsc::channel();
        lock(&self.listeners).push(tx);
        rx
    }

    pub(crate) fn publish(&self, event: ExtensionEvent) {
        let mut listeners = lock(&self.listeners);
        listeners.retain(|tx| tx.send(event.clone()).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_delivers_to_subscribers() {
        let bus = ExtensionEventBus::new();
        let rx = bus.subscribe();
        bus.publish(ExtensionEvent {
            extension: "xkb".to_string(),
            event: "state-changed".to_string(),
            payload: "{}".to_string(),
        });
        let got = rx.recv().unwrap();
        assert_eq!(got.extension, "xkb");
        assert_eq!(got.event, "state-changed");
        assert_eq!(got.payload, "{}");
    }
}
