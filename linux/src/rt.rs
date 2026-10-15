use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("net")
        .enable_all()
        .build()
        .expect("tokio runtime")
});

pub fn runtime() -> &'static tokio::runtime::Runtime {
    &RUNTIME
}

pub async fn run<F, T>(future: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    match RUNTIME.spawn(future).await {
        Ok(value) => value,
        Err(err) => std::panic::resume_unwind(err.into_panic()),
    }
}

pub async fn blocking<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match RUNTIME.spawn_blocking(f).await {
        Ok(value) => value,
        Err(err) => std::panic::resume_unwind(err.into_panic()),
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    RUNTIME.block_on(future)
}

pub async fn sleep(duration: Duration) {
    gtk::glib::timeout_future(duration).await
}

pub fn spawn_local<F: Future<Output = ()> + 'static>(future: F) {
    gtk::glib::spawn_future_local(future);
}
