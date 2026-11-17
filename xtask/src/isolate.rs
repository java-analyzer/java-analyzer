//! Runs a closure on its own thread so that a panic or an endless loop in it cannot take the whole
//! run down.

use std::{
    any::Any,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::mpsc,
    thread,
    time::Duration,
};

/// Stack size of the worker threads. It equals the default stack of a Rust test thread (2 MiB),
/// so a file that parses here parses in the real server's worker threads too.
const STACK_SIZE: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub enum Isolated<T> {
    Done(T),
    /// The closure panicked; carries the panic message.
    Panicked(String),
    /// The closure did not finish in time. Its thread cannot be stopped and keeps running until
    /// the process exits.
    TimedOut,
}

/// Silences the default panic message, so that panics caught by [`run_isolated`] are reported in
/// the run's report only and do not flood the terminal.
pub fn silence_panics() {
    std::panic::set_hook(Box::new(|_| {}));
}

pub fn run_isolated<T: Send + 'static>(
    timeout: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Isolated<T> {
    let (sender, receiver) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("conformance-file".to_string())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(f));
            // The receiver is gone if the run already gave up on this thread.
            let _ = sender.send(result);
        });
    if let Err(e) = spawned {
        return Isolated::Panicked(format!("could not start a worker thread: {e}"));
    }
    match receiver.recv_timeout(timeout) {
        Ok(Ok(value)) => Isolated::Done(value),
        Ok(Err(payload)) => Isolated::Panicked(panic_message(payload)),
        Err(_) => Isolated::TimedOut,
    }
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic with a non-string payload".to_string()
    }
}
