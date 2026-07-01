use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Clone, Default)]
pub(crate) struct RunCancellationRegistry {
    active: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
}

#[derive(Debug)]
pub(crate) struct RunCancellationToken {
    receiver: watch::Receiver<bool>,
}

impl RunCancellationRegistry {
    pub(crate) fn register(&self, run_id: &str) -> RunCancellationToken {
        let (sender, receiver) = watch::channel(false);
        self.active
            .lock()
            .expect("run cancellation registry lock poisoned")
            .insert(run_id.to_string(), sender);
        RunCancellationToken { receiver }
    }

    pub(crate) fn cancel(&self, run_id: &str) -> bool {
        self.active
            .lock()
            .expect("run cancellation registry lock poisoned")
            .get(run_id)
            .map(|sender| sender.send(true).is_ok())
            .unwrap_or(false)
    }

    pub(crate) fn unregister(&self, run_id: &str) {
        self.active
            .lock()
            .expect("run cancellation registry lock poisoned")
            .remove(run_id);
    }
}

impl RunCancellationToken {
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }
}
