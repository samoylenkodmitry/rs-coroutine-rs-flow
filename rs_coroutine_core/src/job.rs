use std::sync::{Arc, Mutex, Weak};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub struct JobHandle {
    inner: Arc<JobInner>,
}

struct JobInner {
    cancelled: AtomicBool,
    children: Mutex<Vec<Weak<JobInner>>>,
}

impl JobHandle {
    pub fn new_root() -> Self {
        Self {
            inner: Arc::new(JobInner {
                cancelled: AtomicBool::new(false),
                children: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn new_child(&self) -> Self {
        let child = JobHandle {
            inner: Arc::new(JobInner {
                cancelled: AtomicBool::new(false),
                children: Mutex::new(Vec::new()),
            }),
        };

        self.inner
            .children
            .lock()
            .unwrap()
            .push(Arc::downgrade(&child.inner));

        child
    }

    pub fn cancel(&self) {
        if self
            .inner
            .cancelled
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let children = self.inner.children.lock().unwrap().clone();
            for child in children {
                if let Some(child) = child.upgrade() {
                    JobHandle { inner: child }.cancel();
                }
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }
}
