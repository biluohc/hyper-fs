use crossbeam_channel::*;
use futures::sync::mpsc;
use hyper::{Chunk, Error};
use num_cpus;

use std::thread::Builder as ThreadBuilder;
use std::io::{self, BufReader, Read};
use std::time::{Duration, Instant};
use std::collections::BinaryHeap;
use std::thread::JoinHandle;
use std::sync::Arc;
use std::fs::File;
use std::fmt;
use std::mem;

pub struct FsPool {
    builder: Builder,
    workers: Vec<(Arc<()>, Sender<Option<TaskBox>>, JoinHandle<()>)>,
}
impl FsPool {
    fn _new(b: Builder) -> Self {
        Self {
            builder: b,
            workers: Vec::new(),
        }
    }
    pub fn new() -> io::Result<Self> {
        Builder::new().build()
    }
    pub fn pool_size(size: usize) -> io::Result<Self> {
        Builder::new().pool_size(size).build()
    }
    pub fn name_prefix<S: Into<String>>(prefix: S) -> io::Result<Self> {
        Builder::new().name_prefix(prefix).build()
    }
    pub fn push<T>(&self, task: T)
    where
        T: Task + 'static + Send,
    {
        let mut task = TaskBox::new(task);
        self.workers
            .as_slice()
            .iter()
            .min_by_key(|w| Arc::strong_count(&w.0))
            .map(|w| {
                task.count = Some(w.0.clone());
                w.1
                    .send(Some(task))
                    .expect("Has't FsPool thread(Receiver).");
            })
            .expect("FsPool is empty");
    }
}
impl Drop for FsPool {
    fn drop(&mut self) {
        self.workers
            .as_slice()
            .iter()
            .for_each(|w| w.1.send(None).unwrap());
    }
}
pub struct Builder {
    name_prefix: Option<String>,
    stack_size: Option<usize>,
    pool_size: usize,
}
impl Builder {
    pub fn new() -> Self {
        Self {
            name_prefix: None,
            stack_size: None,
            pool_size: num_cpus::get(),
        }
    }
    pub fn name_prefix<S: Into<String>>(mut self, name_prefix: S) -> Self {
        self.name_prefix = Some(name_prefix.into());
        self
    }
    pub fn stack_size(mut self, stack_size: usize) -> Self {
        self.stack_size = Some(stack_size);
        self
    }
    pub fn pool_size(mut self, pool_size: usize) -> Self {
        self.pool_size = pool_size;
        self
    }
    pub fn build(self) -> io::Result<FsPool> {
        assert!(self.pool_size > 0);
        let mut pool = FsPool::_new(self);
        for idx in 0..pool.builder.pool_size {
            let mut b = ThreadBuilder::new();
            if let Some(name) = pool.builder.name_prefix.as_ref() {
                b = b.name(format!("{}{}", name, idx));
            }
            if let Some(size) = pool.builder.stack_size {
                b = b.stack_size(size);
            }
            let (mut worker, sender) = Worker::new();
            let handle = b.spawn(move || worker.work())?;
            let arc = Arc::new(());
            pool.workers.push((arc, sender, handle));
        }
        Ok(pool)
    }
}

struct Worker {
    mc: Receiver<Option<TaskBox>>,
    tasks: TaskTree,
    buf: [u8; 8192],
}

impl Worker {
    fn new() -> (Self, Sender<Option<TaskBox>>) {
        let (mp, mc) = unbounded();
        let new = Self {
            tasks: TaskTree::with_capacity(1024),
            buf: unsafe { mem::uninitialized() },
            mc: mc,
        };
        (new, mp)
    }
    fn work(&mut self) {
        loop {
            match self.tasks.try_call(&mut self.buf[..]) {
                Some(time) => match self.mc.recv_timeout(time) {
                    Ok(task) => match task {
                        Some(t) => {
                            self.tasks.push_taskbox(t);
                        }
                        None => return self.finish(),
                    },
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => return self.finish(),
                },
                None => match self.mc.recv() {
                    Ok(task) => match task {
                        Some(t) => {
                            self.tasks.push_taskbox(t);
                        }
                        None => return self.finish(),
                    },
                    _ => return self.finish(),
                },
            }
        }
    }
    fn finish(&mut self) {
        // todu: finish all task
    }
}
pub struct FileRead {
    file: BufReader<File>,
    sender: mpsc::Sender<Result<Chunk, Error>>,
    full_tmp: Option<Result<Chunk, Error>>,    // store element when SenderError.full
    // buf: [u8; 16384],                       // 16384, 8k/16k, fsblock
    usetime: Instant,
    timeout: Duration,
}
impl fmt::Debug for FileRead {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FileRead")
            .field("file", &self.file)
            .field("sender", &self.sender)
            .field("full_tmp", &self.full_tmp)
            .field("usetime", &self.usetime)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl Task for FileRead {
    fn try_call(&mut self, buf: &mut [u8]) -> Option<Instant> {
        match self.nb_read(buf) {
            Some(Some(t)) => Some(Instant::now() + (t -t/8)),
            Some(None) => Some(Instant::now() + self.timeout),
            None => None,
        }
    }
}
impl FileRead {
    pub fn new(file: File) -> (Self, mpsc::Receiver<Result<Chunk, Error>>) {
        let (mp, sc) = mpsc::channel::<Result<Chunk, Error>>(64);
        let new = Self {
            file: BufReader::new(file),
            sender: mp,
            full_tmp: None,
            // buf: unsafe { mem::uninitialized() },
            usetime: Instant::now(),
            timeout: Duration::from_millis(1),
        };
        (new, sc)
    }
    pub fn nb_read(&mut self, buf: &mut [u8]) -> Option<Option<Duration>> {
        let mut full_tmp = mem::replace(&mut self.full_tmp, None);
        let timeout = self.usetime.elapsed();
        loop {
            match full_tmp {
                Some(data) => {
                    let finish = data.is_err();
                    if let Err(e) = self.sender.try_send(data) {
                        if e.is_full() {
                            self.full_tmp = Some(e.into_inner());
                            return Some(None);
                        }
                        // receiver break connection
                    }
                    // is_err, should break connection
                    if finish {
                        return None;
                    }
                    full_tmp = None;
                }
                None => {
                    match self.file.read(buf) {
                        Ok(len_) => {
                            if len_ == 0 {
                                return None; // finish
                            }
                            let chunk = Chunk::from((buf[0..len_]).to_vec());
                            if let Err(e) = self.sender.try_send(Ok(chunk)) {
                                if e.is_full() {
                                    self.full_tmp = Some(e.into_inner());
                                    self.usetime = Instant::now();
                                    return Some(Some(timeout));
                                }
                                // receiver break connection
                                return None;
                            }
                        }
                        Err(e) => {
                            if let Err(e) = self.sender.try_send(Err(Error::Io(e))) {
                                if e.is_full() {
                                    self.full_tmp = Some(e.into_inner());
                                    self.usetime = Instant::now();
                                    return Some(Some(timeout));
                                }
                                // receiver break connection
                            }
                            // send error finish
                            return None;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct TaskTree(BinaryHeap<TaskBox>);

impl TaskTree {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(cap: usize) -> Self {
        TaskTree(BinaryHeap::with_capacity(cap))
    }
    pub fn push<T>(&mut self, t: T)
    where
        T: Task + Send + 'static,
    {
        self.0.push(TaskBox::new(t))
    }
    pub fn push_taskbox(&mut self, t: TaskBox) {
        self.0.push(t)
    }
    pub fn try_call(&mut self, buf: &mut [u8]) -> Option<Duration> {
        loop {
            let now = Instant::now();
            match self.0.peek() {
                Some(task) => {
                    if task.time > now {
                        return Some(task.time - now); // all block
                    }
                }
                None => return None, //empty
            }
            let mut task = self.0.pop().unwrap();
            // next call
            if task.try_call(buf) {
                self.0.push(task);
            }
        }
    }
}

#[derive(Debug)]
pub struct TaskBox {
    time: Instant,
    task: Box<Task + Send + 'static>,
    count: Option<Arc<()>>,
}

pub trait Task: fmt::Debug {
    fn try_call(&mut self, buf: &mut [u8]) -> Option<Instant>;
}

impl TaskBox {
    pub fn new<T>(t: T) -> Self
    where
        T: Task + 'static + Send,
    {
        Self::with_duration(t, &Duration::from_secs(0))
    }
    pub fn with_duration<T>(t: T, timeout: &Duration) -> Self
    where
        T: Task + 'static + Send,
    {
        Self {
            task: Box::new(t) as Box<Task + Send + 'static>,
            time: Instant::now() + *timeout,
            count: None,
        }
    }
    fn try_call(&mut self, buf: &mut [u8]) -> bool {
        match self.task.try_call(buf) {
            Some(t) => {
                self.time = t;
                true
            }
            None => false,
        }
    }
}
use std::cmp;
impl Eq for TaskBox {} //...
impl PartialEq for TaskBox {
    fn eq(&self, other: &TaskBox) -> bool {
        self.time == other.time
    }
}
impl PartialOrd for TaskBox {
    fn partial_cmp(&self, other: &TaskBox) -> Option<cmp::Ordering> {
        Some(self.cmp(&other))
    }
}
impl Ord for TaskBox {
    fn cmp(&self, other: &TaskBox) -> cmp::Ordering {
        use std::cmp::Ordering;
        match self.time.cmp(&other.time) {
            Ordering::Less => Ordering::Greater,
            Ordering::Greater => Ordering::Less,
            Ordering::Equal => Ordering::Equal,
        }
    }
}
