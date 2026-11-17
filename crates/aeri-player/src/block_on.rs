//! Waits for a future on a thread that has no runtime (ADR 0025).

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls `future` on this thread, parking it between polls until the future's waker fires.
///
/// Correct for futures that wake without a runtime, such as `tokio::sync` channels.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        thread::park();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use tokio::sync::{mpsc, oneshot};

    use super::*;

    #[test]
    fn a_ready_future_returns_at_once() {
        assert_eq!(block_on(async { 7 }), 7);
    }

    #[test]
    fn a_future_woken_from_another_thread_completes() {
        let (send, receive) = oneshot::channel();
        let sender = thread::Builder::new()
            .name("test-sender".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(20));
                send.send("done").expect("the receiver is waiting");
            })
            .expect("the thread starts");

        assert_eq!(block_on(receive), Ok("done"));
        sender.join().expect("the sender did not panic");
    }

    #[test]
    fn a_future_that_pends_more_than_once_is_polled_again_each_time() {
        let (send, mut receive) = mpsc::channel(1);
        let sender = thread::Builder::new()
            .name("test-sender".into())
            .spawn(move || {
                for value in 0..3 {
                    thread::sleep(Duration::from_millis(10));
                    send.blocking_send(value).expect("the receiver is waiting");
                }
            })
            .expect("the thread starts");

        let received = block_on(async {
            let mut all = Vec::new();
            while let Some(value) = receive.recv().await {
                all.push(value);
            }
            all
        });

        assert_eq!(received, [0, 1, 2]);
        sender.join().expect("the sender did not panic");
    }
}
