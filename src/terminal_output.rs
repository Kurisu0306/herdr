//! Ordered writes to the host terminal that never block the client loop.
//!
//! The foreground client handles input, server messages and presentation on one
//! thread. A blocking write to the host terminal stalls all of that: when the
//! host stops reading the pty (code-server pauses a terminal once its browser
//! falls 100000 characters behind on acknowledgements), clicks and keys pile up
//! unprocessed and then run all at once when the host resumes.
//!
//! While the queue runs, writes are appended to it in order and a writer thread
//! drains it to stdout, so the loop keeps going. The client skips frames while
//! earlier output is still pending and presents the latest one when the queue
//! drains. Before `start` and after `finish`, writes go straight to
//! `std::io::stdout()`.

use std::io::{self, Write};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Writers wait once this much output is queued, so a terminal that stays
/// paused cannot grow the queue without bound. Composed frames are skipped
/// rather than queued, so only raw passthrough output can reach this.
const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;

/// Handle that writes through the queue while it runs and to stdout otherwise.
pub(crate) struct Stdout;

pub(crate) fn stdout() -> Stdout {
    Stdout
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if global().push(buf) {
            Ok(buf.len())
        } else {
            io::stdout().write(buf)
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        if global().push(buf) {
            Ok(())
        } else {
            io::stdout().write_all(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if global().running() {
            Ok(())
        } else {
            io::stdout().flush()
        }
    }
}

/// Moves stdout writes onto a writer thread. When the thread cannot start,
/// writes keep going straight to stdout.
pub(crate) fn start() {
    if let Err(error) = global().start(io::stdout()) {
        tracing::warn!(%error, "terminal output queue unavailable; writing to the terminal directly");
    }
}

/// Waits up to `timeout` for queued output to reach the terminal, then stops
/// the writer thread so writes go straight to stdout again. Returns false when
/// the terminal did not take the output in time; the queue then keeps running
/// and direct writes to stdout would block.
pub(crate) fn finish(timeout: Duration) -> bool {
    global().finish(timeout)
}

/// Returns true while earlier output has not reached the terminal yet. In that
/// case `drained` completes once it has.
pub(crate) fn defer_until_drained() -> bool {
    global().defer_until_drained()
}

/// Completes after the queue drains following a `defer_until_drained` that
/// returned true.
pub(crate) async fn drained() {
    global().drained.notified().await;
}

/// Returns whether a queued write failed since the last call, so the caller
/// can repaint from scratch.
pub(crate) fn take_write_failed() -> bool {
    std::mem::take(&mut global().lock().write_failed)
}

fn global() -> &'static Queue {
    static QUEUE: OnceLock<Queue> = OnceLock::new();
    QUEUE.get_or_init(Queue::default)
}

#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    /// Signalled whenever bytes, room or the running state change.
    changed: Condvar,
    drained: tokio::sync::Notify,
}

#[derive(Default)]
struct QueueState {
    running: bool,
    closing: bool,
    bytes: Vec<u8>,
    writing: bool,
    notify_when_drained: bool,
    write_failed: bool,
}

impl QueueState {
    fn pending(&self) -> bool {
        self.writing || !self.bytes.is_empty()
    }
}

impl Queue {
    fn lock(&self) -> MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, state: MutexGuard<'a, QueueState>) -> MutexGuard<'a, QueueState> {
        self.changed
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn running(&self) -> bool {
        self.lock().running
    }

    fn start<W: Write + Send + 'static>(&'static self, terminal: W) -> io::Result<()> {
        let mut state = self.lock();
        if state.running {
            return Ok(());
        }
        crate::thread_spawn::spawn_named("herdr-term-out", move || self.write_queued(terminal))?;
        state.running = true;
        state.closing = false;
        Ok(())
    }

    fn push(&self, buf: &[u8]) -> bool {
        let mut state = self.lock();
        while state.running && !state.closing && state.bytes.len() >= MAX_QUEUED_BYTES {
            state = self.wait(state);
        }
        if !state.running {
            return false;
        }
        state.bytes.extend_from_slice(buf);
        self.changed.notify_all();
        true
    }

    fn defer_until_drained(&self) -> bool {
        let mut state = self.lock();
        let pending = state.running && state.pending();
        if pending {
            state.notify_when_drained = true;
        }
        pending
    }

    fn finish(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        if !state.running {
            return true;
        }
        while state.pending() {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        state.closing = true;
        self.changed.notify_all();
        while state.running {
            state = self.wait(state);
        }
        true
    }

    fn write_queued(&self, mut terminal: impl Write) {
        loop {
            let chunk = {
                let mut state = self.lock();
                while state.bytes.is_empty() && !state.closing {
                    state = self.wait(state);
                }
                if state.bytes.is_empty() {
                    state.running = false;
                    state.closing = false;
                    self.changed.notify_all();
                    return;
                }
                state.writing = true;
                let chunk = std::mem::take(&mut state.bytes);
                // Writers waiting for room can go on.
                self.changed.notify_all();
                chunk
            };
            let result = terminal.write_all(&chunk).and_then(|()| terminal.flush());
            let mut state = self.lock();
            state.writing = false;
            if let Err(error) = result {
                if !state.write_failed {
                    tracing::warn!(%error, bytes = chunk.len(), "failed to write queued terminal output");
                }
                state.write_failed = true;
            }
            if !state.pending() {
                if std::mem::take(&mut state.notify_when_drained) {
                    self.drained.notify_one();
                }
                self.changed.notify_all();
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::os::fd::FromRawFd as _;

    fn pipe() -> (std::fs::File, std::fs::File) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        unsafe {
            (
                std::fs::File::from_raw_fd(fds[0]),
                std::fs::File::from_raw_fd(fds[1]),
            )
        }
    }

    fn leaked_queue() -> &'static Queue {
        Box::leak(Box::default())
    }

    #[test]
    fn writes_reach_the_terminal_in_order_and_finish_returns_to_direct_writes() {
        let queue = leaked_queue();
        let (mut reader, writer) = pipe();
        queue.start(writer).expect("writer thread starts");

        for part in [&b"first "[..], b"second ", b"third"] {
            assert!(queue.push(part));
        }
        assert!(queue.finish(Duration::from_secs(5)));

        assert!(!queue.running());
        assert!(!queue.push(b"direct"), "a finished queue hands writes back");
        let mut written = String::new();
        reader.read_to_string(&mut written).unwrap();
        assert_eq!(written, "first second third");
    }

    #[test]
    fn a_terminal_that_stops_reading_does_not_block_writers_and_drained_fires_after_it_resumes() {
        let queue = leaked_queue();
        let (mut reader, writer) = pipe();
        queue.start(writer).expect("writer thread starts");

        // Far more than a pipe holds, so the writer thread blocks.
        let payload = vec![b'x'; 4 * 1024 * 1024];
        let started = Instant::now();
        assert!(queue.push(&payload));
        assert!(queue.push(b"tail"));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "pushing must not wait for the terminal"
        );
        std::thread::sleep(Duration::from_millis(50));
        assert!(queue.defer_until_drained(), "output is still pending");
        assert!(
            !queue.finish(Duration::from_millis(50)),
            "finish gives up while the terminal is not reading"
        );
        assert!(
            queue.running(),
            "a finish that gave up leaves the queue running"
        );

        let drain = std::thread::spawn(move || {
            let mut sink = Vec::new();
            reader.read_to_end(&mut sink).unwrap();
            sink.len()
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(10), queue.drained.notified()).await
            })
            .expect("drained fires once the terminal reads again");
        assert!(
            !queue.defer_until_drained(),
            "nothing is pending after the drain"
        );

        assert!(queue.finish(Duration::from_secs(5)));
        assert_eq!(drain.join().unwrap(), payload.len() + 4);
    }

    #[test]
    fn a_full_queue_makes_writers_wait_for_room() {
        let queue = leaked_queue();
        let (mut reader, writer) = pipe();
        queue.start(writer).expect("writer thread starts");

        let chunk = vec![b'y'; MAX_QUEUED_BYTES];
        // The writer thread takes the first chunk and blocks on the pipe; the
        // second fills the queue; the third must wait.
        assert!(queue.push(&chunk));
        while !queue.lock().writing {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(queue.push(&chunk));
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let third = std::thread::spawn(move || {
            assert!(queue.push(b"z"));
            done_tx.send(()).unwrap();
        });
        assert!(
            done_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a writer waits while the queue is full"
        );

        let drain = std::thread::spawn(move || {
            let mut sink = Vec::new();
            reader.read_to_end(&mut sink).unwrap();
            sink.len()
        });
        done_rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the writer goes on once the queue has room");
        third.join().unwrap();
        assert!(queue.finish(Duration::from_secs(20)));
        assert_eq!(drain.join().unwrap(), 2 * MAX_QUEUED_BYTES + 1);
    }

    #[test]
    fn a_failed_write_is_reported_once() {
        let queue = leaked_queue();
        let (reader, writer) = pipe();
        // Rust programs ignore SIGPIPE, so writing to a pipe without a reader
        // fails with EPIPE.
        drop(reader);
        queue.start(writer).expect("writer thread starts");
        assert!(queue.push(b"lost"));
        assert!(queue.finish(Duration::from_secs(5)));

        assert!(std::mem::take(&mut queue.lock().write_failed));
        assert!(!std::mem::take(&mut queue.lock().write_failed));
    }
}
