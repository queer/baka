use futures::{
    future::{BoxFuture, FutureExt},
    task::{waker_ref, ArcWake},
};
use std::{
    cell::RefCell,
    future::Future,
    sync::mpsc::{sync_channel, Receiver, SyncSender},
    sync::{Arc, Mutex},
    task::Context,
};

struct Executor<T> {
    result: RefCell<Option<T>>,
    ready_queue: Receiver<Arc<Task<T>>>,
}

#[derive(Clone)]
struct Spawner<T> {
    task_sender: SyncSender<Arc<Task<T>>>,
}

struct Task<T> {
    future: Mutex<Option<BoxFuture<'static, T>>>,

    task_sender: SyncSender<Arc<Task<T>>>,
}

pub fn spawn<T>(future: impl Future<Output = T> + 'static + Send) -> T {
    const MAX_QUEUED_TASKS: usize = 1;
    let (task_sender, ready_queue) = sync_channel(MAX_QUEUED_TASKS);
    let (e, s) = (
        Executor {
            ready_queue,
            result: RefCell::new(None),
        },
        Spawner { task_sender },
    );
    s.spawn(future);
    e.run();
    e.result
        .take()
        .unwrap_or_else(|| panic!("executor had no result"))
}

impl<T> Spawner<T> {
    fn spawn(&self, future: impl Future<Output = T> + 'static + Send) {
        let future = future.boxed();
        let task = Arc::new(Task {
            future: Mutex::new(Some(future)),
            task_sender: self.task_sender.clone(),
        });
        self.task_sender.send(task).expect("too many tasks queued");
    }
}

impl<T> ArcWake for Task<T> {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        let cloned = arc_self.clone();
        arc_self
            .task_sender
            .send(cloned)
            .expect("too many tasks queued");
    }
}

impl<T> Executor<T> {
    fn run(&self) {
        while let Ok(task) = self.ready_queue.recv() {
            let mut future_slot = task.future.lock().unwrap();
            if let Some(mut future) = future_slot.take() {
                let waker = waker_ref(&task);
                let context = &mut Context::from_waker(&waker);

                match future.as_mut().poll(context) {
                    std::task::Poll::Ready(val) => {
                        self.result.replace(Some(val));
                        break;
                    }
                    std::task::Poll::Pending => {
                        *future_slot = Some(future);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = spawn(async { 1 + 2 });
        assert_eq!(result, 3);
    }

    #[test]
    fn it_returns_a_struct() {
        struct Foo {
            a: u32,
            b: u32,
        }

        let result = spawn(async { Foo { a: 1, b: 2 } });
        assert_eq!(result.a, 1);
        assert_eq!(result.b, 2);
    }
}
