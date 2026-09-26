#[cfg(all(not(feature = "async-io"), not(feature = "tokio")))]
compile_error!(r#"Either "tokio" (default) or "async-io" must be enabled."#);

#[cfg(all(feature = "async-io", feature = "tokio"))]
compile_error!(r#"Features "tokio" and "async-io" cannot be enabled at the same time."#);

#[cfg(feature = "tokio")]
mod tokio {
    use std::future::Future;

    pub use tokio::select;
    pub use tokio::sync::Mutex;
    pub use tokio::task_local;

    // remove the return value to compat with async-io
    pub fn spawn<F>(future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(future);
    }

    #[cfg(feature = "blocking")]
    pub fn block_on<T>(future: impl Future<Output = T>) -> T {
        use std::sync::LazyLock;
        use tokio::runtime::Runtime;
        static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
        });
        RUNTIME.block_on(future)
    }

    pub mod mpsc {
        pub use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
    }
    pub mod oneshot {
        pub use tokio::sync::oneshot::{channel, Receiver, Sender};
    }
}
#[cfg(feature = "tokio")]
pub use tokio::*;

#[cfg(all(feature = "async-io", not(feature = "tokio")))]
mod async_io {
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_executor::Executor;
    use std::sync::LazyLock;

    pub use task_local::task_local;

    // Do NOT use async_lock::OnceCell instead
    // the spawn method may be called in async context
    // and async_lock::OnceCell::get_or_init_blocking may result in deadlocks
    struct ExecutorState {
        executor: Executor<'static>,
        driver_running: AtomicBool,
    }

    struct DriverGuard {
        state: &'static ExecutorState,
    }

    impl Drop for DriverGuard {
        fn drop(&mut self) {
            self.state.driver_running.store(false, Ordering::Release);
            if !self.state.executor.is_empty() {
                self.state.kick_driver();
            }
        }
    }

    impl ExecutorState {
        fn get() -> &'static Self {
            static STATE: LazyLock<ExecutorState> = LazyLock::new(|| ExecutorState {
                executor: Executor::new(),
                driver_running: AtomicBool::new(false),
            });
            &STATE
        }

        fn kick_driver(&'static self) {
            if self
                .driver_running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }

            std::thread::spawn(move || {
                let _guard = DriverGuard { state: self };
                block_on(async {
                    while !self.executor.is_empty() {
                        self.executor.tick().await;
                    }
                })
            });
        }
    }

    pub use async_io::block_on;
    pub use async_lock::Mutex;

    pub fn spawn<F>(future: F)
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let state = ExecutorState::get();
        // Queue the task first so a freshly spawned driver never observes an empty executor.
        state.executor.spawn(future).detach();
        state.kick_driver();
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! select {
        ($($patten:pat = $exp:expr => $blk:block)*) => {
            futures_util::select! {
                $( v = $exp => {
                    let $patten = v else { continue };
                    $blk
                } )*
            }
        };
    }
    pub use crate::select;

    pub mod mpsc {
        use futures_util::StreamExt;

        pub use futures_channel::mpsc::TrySendError as SendError;

        pub fn unbounded_channel<T>() -> (UnboundedSender<T>, UnboundedReceiver<T>) {
            let (tx, rx) = futures_channel::mpsc::unbounded();
            (UnboundedSender(tx), UnboundedReceiver(rx))
        }

        pub struct UnboundedSender<T>(futures_channel::mpsc::UnboundedSender<T>);
        impl<T> UnboundedSender<T> {
            pub fn send(&self, value: T) -> Result<(), SendError<T>> {
                self.0.unbounded_send(value)
            }
            pub fn is_closed(&self) -> bool {
                self.0.is_closed()
            }
        }
        impl<T> Clone for UnboundedSender<T> {
            fn clone(&self) -> Self {
                UnboundedSender(self.0.clone())
            }
        }

        pub struct UnboundedReceiver<T>(futures_channel::mpsc::UnboundedReceiver<T>);
        impl<T> UnboundedReceiver<T> {
            pub fn recv(
                &mut self,
            ) -> futures_util::stream::Next<'_, futures_channel::mpsc::UnboundedReceiver<T>>
            {
                self.0.next()
            }
        }
    }

    pub mod oneshot {
        pub use futures_channel::oneshot::{channel, Receiver, Sender};
        //    use std::future::Future;
        //
        //    pub use async_channel::Sender;
        //    pub fn channel<T>() -> (
        //        Sender<T>,
        //        Receiver<T>,
        //    ) {
        //        // The concurrent-queue that used by async-channel has
        //        // single-capacity optimization, performace is fine
        //        let (tx, rx) = async_channel::bounded(1);
        //        let rx = async move { rx.recv().await };
        //        (tx, rx)
        //    }
        //    pub type Receiver<T> = impl Future<Output = Result<T, async_channel::RecvError>>;
    }

}
#[cfg(all(feature = "async-io", not(feature = "tokio")))]
pub use async_io::*;
