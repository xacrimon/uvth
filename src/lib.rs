//! uvth is a library that provides a efficient threadpool as an alternative to the threadpool crate.
//!
//! uvth is more efficient and has less overhead. Benchmarks can be found in the README.

#[macro_use]
extern crate log;

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::sync::atomic::{Ordering, AtomicUsize};

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
    sender: Arc<Sender<Message>>,
    receiver: Arc<Receiver<Message>>,
}

impl MessageQueue {
    fn new() -> Self {
        let (tx, rx) = unbounded();
        let (tx, rx) = (Arc::new(tx), Arc::new(rx));
        Self {
            sender: tx,
            receiver: rx,
        }
    }

    fn insert(&self, message: Message) {
        if self.sender.send(message).is_ok() {
            debug!("Successfully inserted message into queue.");
        } else {
            warn!("Failed to insert message into queue.");
        }
    }

    fn remove(&self) -> Option<Message> {
        if let Ok(message) = self.receiver.recv() {
            debug!("Successfully removed message from queue.");
            Some(message)
        } else {
            warn!("Failed to remove message from queue.");
            None
        }
    }
}

struct Worker {
    queue: MessageQueue,
    notify_exit: Arc<Sender<()>>,
    normal_exit: bool,
}

impl Worker {
    fn start(queue: &MessageQueue, notify_exit: &Arc<Sender<()>>) {
        let queue = queue.clone();
        let notify_exit = notify_exit.clone();
        let mut worker = Worker {
            queue,
            notify_exit,
            normal_exit: false,
        };
        thread::spawn(move || {
            worker.do_work();
        });
    }

    fn do_work(&mut self) {
        debug!("Worker thread started.");
        while let Some(message) = self.queue.remove() {
            match message {
                Message::Task(task) => task.run(),
                Message::Exit => break,
            }
        }
        let _ = self.notify_exit.send(());
        self.normal_exit = true;
        debug!("Worker thread exited.");
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if !self.normal_exit {
            warn!("Panic in threadpool. Restarting worker.");
            Worker::start(&self.queue, &self.notify_exit);
        }
    }
}

/// A somewhat basic but efficient implementation of a threadpool. A threadpool is a classic primitive for parallel computation.
/// It manages a set of worker threads that you can spawn tasks on. The pool manages scheduling of those tasks so you don't have to
/// think about it.
#[derive(Clone)]
pub struct ThreadPool {
    worker_count: Arc<AtomicUsize>,
    queue: MessageQueue,
    notify_exit: Arc<Receiver<()>>,
    notify_exit_tx: Arc<Sender<()>>,
}

impl ThreadPool {
    /// Create a new threadpool with a set number of threads.
    pub fn new(worker_count: usize) -> Self {
        debug!("Creating threadpool");
        let queue = MessageQueue::new();
        let (notify_exit_tx, notify_exit_rx) = unbounded();
        let (notify_exit_tx, notify_exit_rx) = (Arc::new(notify_exit_tx), Arc::new(notify_exit_rx));

        for _ in 0..worker_count {
            Worker::start(&queue, &notify_exit_tx);
        }

        Self {
            worker_count: Arc::new(AtomicUsize::new(worker_count)),
            queue,
            notify_exit: notify_exit_rx,
            notify_exit_tx,
        }
    }

    /// Execute a task on the pool.
    #[inline]
    pub fn execute<F: 'static + FnOnce() + Send>(&self, f: F) {
        let task = Box::new(f);
        self.queue.insert(Message::Task(task));
    }

    /// Alter the amount of worker threads in the pool.
    pub fn set_num_threads(&self, worker_count: usize) {
        self.worker_count.store(worker_count, Ordering::SeqCst);
        self.terminate();
        for _ in 0..worker_count {
            Worker::start(&self.queue, &self.notify_exit_tx);
        }
    }

    /// Terminate all threads in the pool.
    pub fn terminate(&self) {
        let worker_count = self.worker_count.load(Ordering::SeqCst);
        for _ in 0..worker_count {
            self.queue.insert(Message::Exit);
        }
    }

    /// Wait for all workers to exit.
    pub fn join(&self) {
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
        debug!("Waiting for threads to exit.");
        self.join();
    }
}
