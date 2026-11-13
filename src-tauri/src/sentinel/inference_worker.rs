//! A dedicated inference thread with one pending slot per lane.
//!
//! Camera inference must never make the assessment UI wait, and a frame
//! that waits behind a slow model is stale by the time it runs. So each
//! lane holds at most one pending job: submitting while one is pending
//! replaces it, and the replaced job resolves `Outcome::Dropped` at once.
//! The thread takes `Lane::Session` work before `Lane::Preview` work.

use std::sync::{Arc, Condvar, Mutex};

use tokio::sync::oneshot;

/// Independent frame streams that must not replace each other's frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lane {
    /// Frames from a live integrity session.
    Session,
    /// Standalone previews (dev picture-in-picture); never evidence.
    Preview,
}

/// What a submitted job resolved to.
#[derive(Debug, PartialEq)]
pub enum Outcome<R> {
    Done(R),
    /// A newer job in the same lane replaced this one before it ran.
    Dropped,
}

type Pending<J, R> = Option<(J, oneshot::Sender<Outcome<R>>)>;

struct Slots<J, R> {
    session: Pending<J, R>,
    preview: Pending<J, R>,
    shutdown: bool,
}

struct Shared<J, R> {
    slots: Mutex<Slots<J, R>>,
    wake: Condvar,
}

pub struct InferenceWorker<J, R> {
    shared: Arc<Shared<J, R>>,
}

impl<J: Send + 'static, R: Send + 'static> InferenceWorker<J, R> {
    /// Start the thread. `process` runs every job that is not dropped, on
    /// that thread, in submission order per lane.
    pub fn spawn(name: &str, mut process: impl FnMut(Lane, J) -> R + Send + 'static) -> Self {
        let shared = Arc::new(Shared {
            slots: Mutex::new(Slots {
                session: None,
                preview: None,
                shutdown: false,
            }),
            wake: Condvar::new(),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || loop {
                let (lane, job, reply) = {
                    let mut slots = worker.slots.lock().unwrap();
                    loop {
                        if slots.shutdown {
                            return;
                        }
                        if let Some((job, reply)) = slots.session.take() {
                            break (Lane::Session, job, reply);
                        }
                        if let Some((job, reply)) = slots.preview.take() {
                            break (Lane::Preview, job, reply);
                        }
                        slots = worker.wake.wait(slots).unwrap();
                    }
                };
                // A panicking model must not take the thread down with it:
                // the submitter sees its reply channel close instead.
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(lane, job)));
                if let Ok(result) = result {
                    // The submitter may have gone away; nothing to tell it.
                    let _ = reply.send(Outcome::Done(result));
                }
            })
            .expect("spawn sentinel inference thread");
        InferenceWorker { shared }
    }

    /// Queue `job` in `lane`, replacing (and dropping) any job still pending
    /// there.
    pub fn submit(&self, lane: Lane, job: J) -> oneshot::Receiver<Outcome<R>> {
        let (tx, rx) = oneshot::channel();
        let replaced = {
            let mut slots = self.shared.slots.lock().unwrap();
            let slot = match lane {
                Lane::Session => &mut slots.session,
                Lane::Preview => &mut slots.preview,
            };
            slot.replace((job, tx))
        };
        if let Some((_, stale)) = replaced {
            let _ = stale.send(Outcome::Dropped);
        }
        self.shared.wake.notify_one();
        rx
    }
}

impl<J, R> Drop for InferenceWorker<J, R> {
    fn drop(&mut self) {
        if let Ok(mut slots) = self.shared.slots.lock() {
            slots.shutdown = true;
        }
        self.shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[tokio::test]
    async fn a_frame_submitted_while_the_model_is_busy_replaces_the_waiting_one() {
        let (started_tx, started_rx) = mpsc::channel::<u32>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let worker = InferenceWorker::spawn("test-inference", move |_lane, job: u32| {
            started_tx.send(job).unwrap();
            release_rx.recv().unwrap();
            job * 10
        });

        let first = worker.submit(Lane::Session, 1);
        assert_eq!(started_rx.recv().unwrap(), 1, "first frame is running");
        let second = worker.submit(Lane::Session, 2);
        let third = worker.submit(Lane::Session, 3);
        assert_eq!(second.await.unwrap(), Outcome::Dropped);

        release_tx.send(()).unwrap();
        assert_eq!(first.await.unwrap(), Outcome::Done(10));
        assert_eq!(started_rx.recv().unwrap(), 3, "the newest frame runs next");
        release_tx.send(()).unwrap();
        assert_eq!(third.await.unwrap(), Outcome::Done(30));
    }

    #[tokio::test]
    async fn a_preview_frame_never_replaces_a_session_frame() {
        let (started_tx, started_rx) = mpsc::channel::<(Lane, u32)>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let worker = InferenceWorker::spawn("test-inference", move |lane, job: u32| {
            started_tx.send((lane, job)).unwrap();
            release_rx.recv().unwrap();
            job
        });

        let busy = worker.submit(Lane::Preview, 0);
        assert_eq!(started_rx.recv().unwrap(), (Lane::Preview, 0));
        let preview = worker.submit(Lane::Preview, 1);
        let session = worker.submit(Lane::Session, 2);
        for _ in 0..3 {
            release_tx.send(()).unwrap();
        }
        assert_eq!(busy.await.unwrap(), Outcome::Done(0));
        assert_eq!(session.await.unwrap(), Outcome::Done(2));
        assert_eq!(preview.await.unwrap(), Outcome::Done(1));
        assert_eq!(
            started_rx.recv().unwrap(),
            (Lane::Session, 2),
            "session first"
        );
        assert_eq!(started_rx.recv().unwrap(), (Lane::Preview, 1));
    }
}
