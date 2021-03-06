extern crate futures;
extern crate tokio_core;
extern crate tokio_io;

use self::futures::{Future, Stream, IntoFuture};
use self::futures::sync::oneshot;
use self::futures::sync::oneshot::{Sender, Receiver};
use self::tokio_core::reactor::{Core, Timeout};

use std::sync::{Arc, Barrier};
use std::thread;
use self::futures::Canceled;

use std::io::Error;
use std::time::Duration;

pub struct StoppableCore {
    core: Core,
    // TOCONSIDER use a lock instead of a barrier?
    barrier: Arc<Barrier>,
    signal: Receiver<i64>,
}

/// Spans a shutdown thread and let it run into a barrier.
/// When the barrier is released, this thread will signal 1 on the Sender channel provided.
/// Shutdown is as complicated, since the oneshot::sender is not clonable / threadsafe ...
fn install_trigger_thread(oneshot_sender_stream: Sender<i64>) -> Arc<Barrier> {
    // barrier awaits the releaser thread and the signalling one
    let barrier = Arc::new(Barrier::new(2));
    let copy_for_thread = barrier.clone();
    // the releaser thread
    let builder = thread::Builder::new()
        .name("barrier to signal thread".into());
    builder.spawn(move || {
        debug!("waiting for end");
        copy_for_thread.wait();
        debug!("signalling end");
        // release the oneshot future blockig core.run()
        oneshot_sender_stream.send(1).unwrap();
    }).unwrap();
    barrier
}

impl StoppableCore {

    pub fn new() -> Result<StoppableCore, Error> {
        // this is the main event loop, powered by tokio core
        let core = Core::new()?;

        let (tx, rx) = oneshot::channel::<i64>();
        let barrier = install_trigger_thread(tx);

        Ok(StoppableCore {
            core: core,
            barrier: barrier,
            signal: rx,
        })
    }

    pub fn stop(& self) {
        self.barrier.wait();
    }

    pub fn run(mut self) -> Result<i64, Canceled> {
        self.core.run(self.signal)
    }

    ///
    /// Run the main event pump with a defined timeout
    /// ``` timeout = Duration::from_secs(timeout) ```
    ///
    pub fn run_timeout(mut self, timeout : Duration) -> Result<i64, Canceled> {

        let fut = Timeout::new(timeout, &self.core.handle()).into_future();
        let timeout = fut.flatten();

        let barrier = self.barrier.clone();
        let timeout_expired = timeout.and_then(move |_|-> Result<(), Error> {
            warn!("timeout expired");
            barrier.wait();
            Ok(())
        });

        self.core.handle().spawn(timeout_expired.into_future().map_err(|_|()));

        self.core.run(self.signal)
    }
}
