//! HTTP connection drivers keep running while an actor executes synchronous guest work.
use std::{future::Future, sync::OnceLock};
use tokio::{runtime::Runtime, task::JoinHandle};

fn runtime() -> Result<&'static Runtime, String> {
    static RUNTIME: OnceLock<Result<Runtime, std::io::Error>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("turbo-surf-http")
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|error| format!("HTTP transport runtime: {error}"))
}

struct Operation<T>(JoinHandle<T>);
impl<T> Drop for Operation<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) async fn run<F, T>(future: F) -> Result<T, String>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let mut operation = Operation(runtime()?.spawn(future));
    (&mut operation.0)
        .await
        .map_err(|error| format!("HTTP transport task: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    #[tokio::test]
    async fn dropping_the_caller_aborts_its_transport_operation() {
        let dropped = Arc::new(AtomicBool::new(false));
        let marker = Dropped(dropped.clone());
        let (started, ready) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(run(async move {
            let marker = marker;
            started.send(()).unwrap();
            std::future::pending::<()>().await;
            drop(marker);
        }));
        ready.await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("caller cancellation must drop the actual transport future");
    }
}
