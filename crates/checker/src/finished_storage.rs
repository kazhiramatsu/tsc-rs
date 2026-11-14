//! Destruction of owned one-shot storage after its result has been published.

use std::sync::{mpsc, OnceLock};

type Storage = Box<dyn Send>;
type Sender = mpsc::Sender<Storage>;

fn start_cleanup() -> Option<Sender> {
    let (sender, receiver) = mpsc::channel::<Storage>();
    let _helper = std::thread::Builder::new()
        .name("tsc-cleanup".into())
        .spawn(move || {
            for storage in receiver {
                drop(storage);
            }
        })
        .ok()?;
    Some(sender)
}

fn send(sender: Option<&Sender>, storage: Storage) {
    let Some(sender) = sender else {
        std::mem::forget(storage);
        return;
    };
    if let Err(rejected) = sender.send(storage) {
        // A failed helper must not put a record walk back on the caller.
        std::mem::forget(rejected.0);
    }
}

pub(crate) fn defer(storage: impl Send + 'static) {
    static CLEANUP: OnceLock<Option<Sender>> = OnceLock::new();
    send(
        CLEANUP.get_or_init(start_cleanup).as_ref(),
        Box::new(storage),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    struct BlockedDrop {
        started: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        finished: mpsc::Sender<()>,
    }

    impl Drop for BlockedDrop {
        fn drop(&mut self) {
            self.started.send(()).unwrap();
            self.release.recv().unwrap();
            self.finished.send(()).unwrap();
        }
    }

    #[test]
    fn submitting_storage_does_not_join_its_destructor() {
        let sender = start_cleanup().expect("cleanup thread");
        let (started, starting) = mpsc::channel();
        let (release, releasing) = mpsc::channel();
        let (finished, finishing) = mpsc::channel();
        send(
            Some(&sender),
            Box::new(BlockedDrop {
                started,
                release: releasing,
                finished,
            }),
        );
        starting.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(
            finishing.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        finishing.recv_timeout(Duration::from_secs(10)).unwrap();
    }

    struct MarkDrop(Arc<AtomicBool>);
    impl Drop for MarkDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[test]
    fn unavailable_helpers_keep_destruction_off_the_sender() {
        let absent = Arc::new(AtomicBool::new(false));
        send(None, Box::new(MarkDrop(absent.clone())));
        assert!(!absent.load(Ordering::Acquire));
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let closed = Arc::new(AtomicBool::new(false));
        send(Some(&sender), Box::new(MarkDrop(closed.clone())));
        assert!(!closed.load(Ordering::Acquire));
    }
}
