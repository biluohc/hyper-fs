///You can choose your love ThreadPool
/// 
///But `futures-cpupool` will let `main` thread stackflow when send a big file on my machine.
/// 
/// wait to debug for it...
pub trait FsPool: Clone + Send + Sync {
    fn push<F>(&self, task: F)
    where
        F: FnOnce() + Send + 'static;
}