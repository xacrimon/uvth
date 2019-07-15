//! uvth is a library that provides a efficient threadpool as an alternative to the threadpool crate.
//!
//! uvth is more efficient and has less overhead than the threadpool crate. Benchmarks can be found in the README.

#[macro_use]
extern crate log;

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

trait Job: Send {
    fn run(self: Box<Self>);
}

impl<F: FnOnce() + Send> Job for F {
    fn run(self: Box<Self>) {
        (*self)();
    }
}

type Task = Box<dyn Job>;

enum Message {
    Task(Task),
    Exit,
}

#[derive(Clone)]
struct MessageQueue {
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

impl MessageQueue {
    fn new() -> Self {
        let (tx, rx) = unbounded();
        Self {
            sender: tx,
            receiver: rx,
        }
    }

    fn insert(&self, message: Message) {
        if self.sender.send(message).is_err() {
            error!("Failed to insert message into queue.");
        }
    }

    fn remove(&self) -> Option<Message> {
        if let Ok(message) = self.receiver.recv() {
            Some(message)
        } else {
            error!("Failed to remove message from queue.");
            None
        }
    }
}

struct Worker {
    queue: MessageQueue,
    notify_exit: Sender<()>,
    normal_exit: bool,
    stats: Arc<Stats>,
    name: Option<String>,
    stack_size: Option<usize>,
}

impl Worker {
    fn start(
        queue: &MessageQueue,
        notify_exit: &Sender<()>,
        stats: &Arc<Stats>,
        name: Option<String>,
        stack_size: Option<usize>,
    ) {
        let queue = queue.clone();
        let notify_exit = notify_exit.clone();
        let stats = stats.clone();
        let mut worker = Worker {
            queue,
            notify_exit,
            normal_exit: false,
            stats,
            name,
            stack_size,
        };

        let mut builder = thread::Builder::new();
        if let Some(name) = worker.name.as_ref() {
            builder = builder.name(name.to_string());
        }
        if let Some(stack_size) = stack_size {
            builder = builder.stack_size(stack_size);
        }
        builder
            .spawn(move || {
                worker.do_work();
                drop(worker);
            })
            .expect("failed to spawn thread");
    }

    fn do_work(&mut self) {
        debug!("Worker thread started.");
        while let Some(message) = self.queue.remove() {
            match message {
                Message::Task(task) => {
                    self.stats.queued_jobs.fetch_sub(1, Ordering::Relaxed);
                    self.stats.active_jobs.fetch_add(1, Ordering::Relaxed);
                    task.run();
                    self.stats.active_jobs.fetch_sub(1, Ordering::Relaxed);
                    self.stats.completed_jobs.fetch_add(1, Ordering::Relaxed);
                }
                Message::Exit => {
                    self.normal_exit = true;
                    break;
                }
            }
        }
        let _ = self.notify_exit.send(());
        debug!("Worker thread exited.");
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !self.normal_exit {
            warn!("Panic in threadpool. Restarting worker.");
            Worker::start(
                &self.queue,
                &self.notify_exit,
                &self.stats,
                self.name.clone(),
                self.stack_size,
            );
        }
    }
}

pub struct Stats {
    pub queued_jobs: AtomicUsize,
    pub active_jobs: AtomicUsize,
    pub completed_jobs: AtomicUsize,
}

impl Stats {
    fn new() -> Self {
        Self {
            queued_jobs: AtomicUsize::new(0),
            active_jobs: AtomicUsize::new(0),
            completed_jobs: AtomicUsize::new(0),
        }
    }
}

/// A factory for configuring and creating a ThreadPool.
pub struct ThreadPoolBuilder {
    num_threads: usize,
    name: Option<String>,
    stack_size: Option<usize>,
}

impl ThreadPoolBuilder {
    /// Create a new factory with default options. Default values are found in the docs for each respective method.
    pub fn new() -> Self {
        Self {
            num_threads: num_cpus::get(),
            name: None,
            stack_size: None,
        }
    }

    /// Set the amount of threads. If you do not manually set this it will have one thread per logical processor.
    pub fn num_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = num_threads;
        self
    }

    /// Set the name of the threads. Not setting this will cause the threads to go unnamed.
    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Set the stack size of the threads. Not setting this will result in platform specific behaviour.
    pub fn stack_size(mut self, stack_size: usize) -> Self {
        self.stack_size = Some(stack_size);
        self
    }

    /// Assemble the threadpool.
    pub fn build(self) -> ThreadPool {
        ThreadPool::create_new(self.num_threads, self.name, self.stack_size)
    }
}

/// A somewhat basic but efficient implementation of a threadpool. A threadpool is a classic primitive for parallel computation.
/// It manages a set of worker threads that you can spawn tasks on. The pool manages scheduling of those tasks so you don't have to
/// think about it.
#[derive(Clone)]
pub struct ThreadPool {
    worker_count: Arc<AtomicUsize>,
    queue: MessageQueue,
    notify_exit: Receiver<()>,
    notify_exit_tx: Sender<()>,
    pub stats: Arc<Stats>,
    name: Option<String>,
    stack_size: Option<usize>,
}

impl ThreadPool {
    /// Create a new threadpool with a set number of threads.
    fn create_new(worker_count: usize, name: Option<String>, stack_size: Option<usize>) -> Self {
        debug!("Creating threadpool");
        let queue = MessageQueue::new();
        let (notify_exit_tx, notify_exit_rx) = unbounded();
        let stats = Arc::new(Stats::new());

        for _ in 0..worker_count {
            Worker::start(&queue, &notify_exit_tx, &stats, name.clone(), stack_size);
        }

        Self {
            worker_count: Arc::new(AtomicUsize::new(worker_count)),
            queue,
            notify_exit: notify_exit_rx,
            notify_exit_tx,
            stats,
            name,
            stack_size,
        }
    }

    /// Execute a task on the pool.
    #[inline]
    pub fn execute<F: 'static + FnOnce() + Send>(&self, f: F) {
        let task = Box::new(f);
        self.queue.insert(Message::Task(task));
        self.stats.queued_jobs.fetch_add(1, Ordering::Relaxed);
    }

    /// Fetches the amount of queued jobs.
    #[inline]
    pub fn queued_jobs(&self) -> usize {
        self.stats.queued_jobs.load(Ordering::SeqCst)
    }

    /// Alter the amount of worker threads in the pool.
    pub fn set_num_threads(&mut self, worker_count: usize) {
        assert_ne!(worker_count, 0);

        self.terminate();
        self.worker_count.store(worker_count, Ordering::SeqCst);
        for _ in 0..worker_count {
            Worker::start(
                &self.queue,
                &self.notify_exit_tx,
                &self.stats,
                self.name.clone(),
                self.stack_size,
            );
        }
    }

    /// Terminate all threads in the pool. Calling execute after this will result in silently ignoring jobs.
    pub fn terminate(&self) {
        let worker_count = self.worker_count.load(Ordering::SeqCst);
        self.worker_count.store(0, Ordering::SeqCst);
        for _ in 0..worker_count {
            self.queue.insert(Message::Exit);
        }
        self.join();
    }

    /// Wait for all workers to exit.
    fn join(&self) {
        let worker_count = self.worker_count.load(Ordering::SeqCst);
        for _ in 0..worker_count {
            let _ = self.notify_exit.recv();
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        debug!("Dropping threadpool.");
        debug!("Terminating threads");
        self.terminate();
    }
}
