#[macro_use]
extern crate log;

use crossbeam_channel::{unbounded, Receiver, Sender};
use std::sync::Arc;
use std::thread;

type Task = Box<dyn FnOnce() + Send>;

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
                Message::Task(task) => task(),
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

pub struct ThreadPool {
    worker_count: usize,
    queue: MessageQueue,
    notify_exit: Receiver<()>,
}

impl ThreadPool {
    pub fn new(worker_count: usize) -> Self {
        debug!("Creating threadpool");
        let queue = MessageQueue::new();
        let (notify_exit_tx, notify_exit_rx) = unbounded();
        let notify_exit_tx = Arc::new(notify_exit_tx);

        for _ in 0..worker_count {
            Worker::start(&queue, &notify_exit_tx);
        }

        Self {
            worker_count,
            queue,
            notify_exit: notify_exit_rx,
        }
    }

    pub fn execute<F: 'static + FnOnce() + Send>(&self, f: F) {
        let task = Box::new(f);
        self.queue.insert(Message::Task(task));
    }

    pub fn terminate(&self) {
        for _ in 0..self.worker_count {
            self.queue.insert(Message::Exit);
        }
    }

    pub fn join(&self) {
        for _ in 0..self.worker_count {
            self.notify_exit.recv().unwrap();
        }
    }
}