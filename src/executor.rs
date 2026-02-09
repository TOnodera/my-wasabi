extern crate alloc;
use crate::hpet::global_timestamp;
use crate::info;
use crate::mutex::Mutex;
use crate::result::Result;
use crate::x86::busy_loop_hint;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::fmt::write;
use core::fmt::Debug;
use core::future::Future;
use core::panic::Location;
use core::pin::Pin;
use core::ptr::null;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;
use core::task::RawWaker;
use core::task::RawWakerVTable;
use core::task::Waker;
use core::time::Duration;

struct Task<T> {
    future: Pin<Box<dyn Future<Output = Result<T>>>>,
    created_at_file: &'static str,
    created_at_line: u32,
}
impl<T> Task<T> {
    #[track_caller]
    fn new(future: impl Future<Output = Result<T>> + 'static) -> Task<T> {
        Task {
            future: Box::pin(future),
            created_at_file: Location::caller().file(),
            created_at_line: Location::caller().line(),
        }
    }
    fn poll(&mut self, context: &mut Context) -> Poll<Result<T>> {
        self.future.as_mut().poll(context)
    }
}

impl<T> Debug for Task<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Task ({}:{})",
            self.created_at_file, self.created_at_line,
        )
    }
}

fn no_op_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        no_op_raw_waker()
    }
    let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(null::<()>(), vtable)
}

fn no_op_waker() -> Waker {
    unsafe { Waker::from_raw(no_op_raw_waker()) }
}

pub struct Executor {
    task_queue: Option<VecDeque<Task<()>>>,
}
impl Executor {
    const fn new() -> Self {
        Self { task_queue: None }
    }
    fn task_queue(&mut self) -> &mut VecDeque<Task<()>> {
        if self.task_queue.is_none() {
            self.task_queue = Some(VecDeque::new());
        }
        self.task_queue.as_mut().unwrap()
    }
    fn enqueue(&mut self, task: Task<()>) {
        self.task_queue().push_back(task);
    }
    pub fn run(executor: &Mutex<Option<Self>>) -> ! {
        info!("Executing starts running...");
        loop {
            let task =
                executor.lock().as_mut().map(|e| e.task_queue().pop_front());
            info!("task is {:?}", task);
            if let Some(Some(mut task)) = task {
                info!("task is poped..");
                let waker = no_op_waker();
                let mut context = Context::from_waker(&waker);
                match task.poll(&mut context) {
                    Poll::Ready(result) => {
                        info!("Task completed: {:?}: {:?}", task, result);
                    }
                    Poll::Pending => {
                        info!("task is pendding...");
                        if let Some(e) = executor.lock().as_mut() {
                            e.task_queue().push_back(task);
                        }
                    }
                }
            }
        }
    }
}
impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct Yield {
    polled: AtomicBool,
}
impl Future for Yield {
    type Output = ();
    fn poll(
        self: Pin<&mut Self>,
        _context: &mut Context,
    ) -> Poll<Self::Output> {
        if self.polled.fetch_or(true, Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

pub async fn yield_execution() {
    Yield::default().await
}

struct TimeoutFuture {
    time_out: Duration,
}
impl TimeoutFuture {
    fn new(duration: Duration) -> Self {
        info!("TimeoutFuture::new()");
        Self {
            time_out: global_timestamp() + duration,
        }
    }
}
impl Future for TimeoutFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        info!(
            "timeout: {:?}, global_timestamp: {:?}",
            self.time_out,
            global_timestamp()
        );
        if self.time_out < global_timestamp() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

pub async fn sleep(duration: Duration) {
    TimeoutFuture::new(duration).await
}

static GLOBAL_EXECUTOR: Mutex<Option<Executor>> = Mutex::new(None);
#[track_caller]
pub fn spawn_global(future: impl Future<Output = Result<()>> + 'static) {
    let task = Task::new(future);
    GLOBAL_EXECUTOR.lock().get_or_insert_default().enqueue(task);
}
pub fn start_global_executor() {
    info!("Starting global executor loop");
    Executor::run(&GLOBAL_EXECUTOR);
}
