use crate::platform;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const PAGE: usize = 80;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum Profile {
    #[default]
    Gentle,
    Balanced,
}
impl Profile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gentle => "轻载优先",
            Self::Balanced => "均衡",
        }
    }
}
pub struct Control {
    stopped: AtomicBool,
    paused: Mutex<bool>,
    wake: Condvar,
    profile: AtomicU8,
}
impl Control {
    fn new(profile: Profile) -> Self {
        Self {
            stopped: AtomicBool::new(false),
            paused: Mutex::new(false),
            wake: Condvar::new(),
            profile: AtomicU8::new(profile as u8),
        }
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.wake.notify_all();
    }
    pub fn pause(&self, value: bool) {
        *self.paused.lock().unwrap_or_else(|e| e.into_inner()) = value;
        self.wake.notify_all();
    }
    pub fn profile(&self, value: Profile) {
        self.profile.store(value as u8, Ordering::Relaxed);
        self.wake.notify_all();
    }
    fn checkpoint(&self) -> bool {
        let mut paused = self.paused.lock().unwrap_or_else(|e| e.into_inner());
        while *paused && !self.stopped.load(Ordering::Relaxed) {
            paused = self.wake.wait(paused).unwrap_or_else(|e| e.into_inner());
        }
        !self.stopped.load(Ordering::Relaxed)
    }
    fn wait(&self, duration: Duration) {
        let guard = self.paused.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self.wake.wait_timeout(guard, duration);
    }
}
#[derive(Clone, Default, Serialize)]
pub struct Stats {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub logical: u64,
    pub errors: u64,
    pub skipped_links: u64,
    pub hardlinks: u64,
    pub estimated: u64,
    pub seconds: f64,
    pub worker_cpu_seconds: f64,
    pub peak_memory_bytes: u64,
    pub complete: bool,
    pub stopped: bool,
}
pub enum Event {
    Progress(Stats),
    Finished(Stats, Option<String>),
}
pub struct Job {
    pub control: Arc<Control>,
    pub events: Receiver<Event>,
    pub path: PathBuf,
    pub worker: thread::JoinHandle<()>,
}
#[derive(Clone)]
pub struct Node {
    pub id: i64,
    pub parent: Option<i64>,
    pub name: String,
    pub directory: bool,
    pub bytes: u64,
    pub logical: u64,
    pub files: u64,
    pub folders: u64,
    pub status: i64,
}
fn node(row: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        id: row.get(0)?,
        parent: row.get(1)?,
        name: row.get(2)?,
        directory: row.get(3)?,
        bytes: row.get::<_, i64>(4)? as u64,
        logical: row.get::<_, i64>(5)? as u64,
        files: row.get::<_, i64>(6)? as u64,
        folders: row.get::<_, i64>(7)? as u64,
        status: row.get(8)?,
    })
}
const FIELDS: &str = "id,parent,name,directory,bytes,logical,files,folders,status";

pub struct Index {
    connection: Connection,
}
impl Index {
    pub fn snapshot<T>(&self, read: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let transaction = self.connection.unchecked_transaction()?;
        let result = read(self);
        drop(transaction);
        result
    }
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_millis(30))?;
        connection.execute_batch("PRAGMA cache_size=-2048; PRAGMA mmap_size=0;")?;
        Ok(Self { connection })
    }
    pub fn get(&self, id: i64) -> Result<Node> {
        Ok(self.connection.query_row(
            &format!("SELECT {FIELDS} FROM nodes WHERE id=?1"),
            [id],
            node,
        )?)
    }
    pub fn list(&self, parent: i64, large: bool, offset: usize) -> Result<Vec<Node>> {
        let filter = if large { "directory=0" } else { "parent=?1" };
        let sql = format!(
            "SELECT {FIELDS} FROM nodes WHERE {filter} ORDER BY bytes DESC,id LIMIT ?2 OFFSET ?3"
        );
        let mut statement = self.connection.prepare_cached(&sql)?;
        let rows = statement.query_map(params![parent, PAGE as i64, offset as i64], node)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn chart(&self, parent: i64) -> Result<Vec<Node>> {
        let mut statement = self.connection.prepare_cached(&format!(
            "SELECT {FIELDS} FROM nodes WHERE parent=?1 ORDER BY bytes DESC,id LIMIT 49"
        ))?;
        Ok(statement
            .query_map([parent], node)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn path(&self, id: i64) -> Result<PathBuf> {
        let (raw, parent, directory): (Vec<u8>, Option<i64>, bool) = self.connection.query_row(
            "SELECT raw_name,parent,directory FROM nodes WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if directory {
            let raw: Vec<u8> = self.connection.query_row(
                "SELECT path FROM directories WHERE id=?1",
                [id],
                |r| r.get(0),
            )?;
            Ok(PathBuf::from(platform::decode(&raw)))
        } else {
            Ok(self
                .path(parent.ok_or("文件缺少父目录")?)?
                .join(platform::decode(&raw)))
        }
    }
    pub fn issues(&self) -> Result<Vec<(String, String)>> {
        let mut statement = self
            .connection
            .prepare("SELECT path,message FROM issues ORDER BY rowid DESC LIMIT 100")?;
        Ok(statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

pub fn cache_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DiskIsle")
}
fn initialize(path: &Path, root: &Path) -> Result<()> {
    let connection = Connection::open(path)?;
    connection.execute_batch("
        PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-8192;
        PRAGMA mmap_size=0; PRAGMA wal_autocheckpoint=512;
        CREATE TABLE nodes(id INTEGER PRIMARY KEY,parent INTEGER,name TEXT NOT NULL,raw_name BLOB NOT NULL,directory INTEGER NOT NULL,bytes INTEGER NOT NULL DEFAULT 0,logical INTEGER NOT NULL DEFAULT 0,files INTEGER NOT NULL DEFAULT 0,folders INTEGER NOT NULL DEFAULT 0,status INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX children ON nodes(parent,bytes DESC,id);
        CREATE INDEX largefiles ON nodes(bytes DESC,id) WHERE directory=0;
        CREATE TABLE directories(id INTEGER PRIMARY KEY,path BLOB NOT NULL,pending INTEGER NOT NULL);
        CREATE INDEX pending_directories ON directories(pending,id);
        CREATE TABLE identities(identity BLOB PRIMARY KEY) WITHOUT ROWID;
        CREATE TABLE issues(path TEXT,message TEXT);
    ")?;
    connection.execute(
        "INSERT INTO nodes(id,name,raw_name,directory) VALUES(1,?1,?2,1)",
        params![root.to_string_lossy(), platform::encode(root.as_os_str())],
    )?;
    connection.execute(
        "INSERT INTO directories VALUES(1,?1,1)",
        [platform::encode(root.as_os_str())],
    )?;
    Ok(())
}
pub fn start(root: PathBuf, profile: Profile) -> Result<Job> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("请选择一个可以访问的磁盘或文件夹".into());
    }
    let cache = cache_root().join("scans");
    std::fs::create_dir_all(&cache)?;
    // Only clean completed old cache files. Active workers may belong to another process.
    if let Ok(entries) = std::fs::read_dir(&cache) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("scan-")
                && entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|d| d.as_secs() > 7 * 86400)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = cache.join(format!("scan-{}-{stamp}.sqlite", std::process::id()));
    initialize(&path, &root)?;
    let control = Arc::new(Control::new(profile));
    let (sender, events) = mpsc::sync_channel(2);
    let worker_control = control.clone();
    let worker_path = path.clone();
    let worker = thread::Builder::new()
        .name("disk-isle-scan".into())
        .spawn(move || {
            let start = Instant::now();
            platform::lower_priority();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scan(&worker_path, &worker_control, &sender)
            }));
            let (mut stats, error) = match result {
                Ok(Ok(stats)) => (stats, None),
                Ok(Err(e)) => (
                    Stats::default(),
                    Some(format!("扫描中断，已写入的数据可继续查看。{e}")),
                ),
                Err(_) => (
                    Stats::default(),
                    Some("扫描线程意外结束，已写入的数据可继续查看".into()),
                ),
            };
            if error.is_some()
                && let Ok(index) = Index::open(&worker_path)
                && let Ok(root) = index.get(1)
            {
                stats.files = root.files;
                stats.folders = root.folders;
                stats.bytes = root.bytes;
                stats.logical = root.logical;
                stats.errors = 1;
            }
            stats.seconds = start.elapsed().as_secs_f64();
            stats.worker_cpu_seconds = platform::cpu_seconds();
            stats.peak_memory_bytes = platform::memory_peak();
            stats.stopped = worker_control.stopped.load(Ordering::Relaxed);
            stats.complete = error.is_none() && !stats.stopped;
            let _ = sender.send(Event::Finished(stats, error));
        })?;
    Ok(Job {
        control,
        events,
        path,
        worker,
    })
}

struct Throttle {
    wall: Instant,
    cpu: f64,
}
impl Throttle {
    fn new() -> Self {
        Self {
            wall: Instant::now(),
            cpu: platform::cpu_seconds(),
        }
    }
    fn yield_if_needed(&mut self, control: &Control) {
        if self.wall.elapsed() < Duration::from_millis(80) {
            return;
        }
        let ratio = if control.profile.load(Ordering::Relaxed) == Profile::Gentle as u8 {
            0.25
        } else {
            0.55
        };
        let cpu = platform::cpu_seconds() - self.cpu;
        let wall = self.wall.elapsed().as_secs_f64();
        let delay = (cpu / ratio - wall).clamp(0.0, 0.4);
        if delay > 0.001 {
            control.wait(Duration::from_secs_f64(delay));
        }
        self.wall = Instant::now();
        self.cpu = platform::cpu_seconds();
    }
}
fn aggregate(
    connection: &Connection,
    parent: i64,
    bytes: u64,
    logical: u64,
    files: u64,
    folders: u64,
) -> Result<()> {
    connection.prepare_cached("WITH RECURSIVE ancestors(id) AS (SELECT ?1 UNION ALL SELECT n.parent FROM nodes n JOIN ancestors a ON n.id=a.id WHERE n.parent IS NOT NULL) UPDATE nodes SET bytes=bytes+?2,logical=logical+?3,files=files+?4,folders=folders+?5 WHERE id IN(SELECT id FROM ancestors)")?.execute(params![parent,i64::try_from(bytes)?,i64::try_from(logical)?,i64::try_from(files)?,i64::try_from(folders)?])?;
    Ok(())
}
fn issue(
    connection: &Connection,
    stats: &mut Stats,
    path: &Path,
    error: impl std::fmt::Display,
) -> Result<()> {
    stats.errors += 1;
    connection.execute(
        "INSERT INTO issues VALUES(?1,?2)",
        params![path.to_string_lossy(), error.to_string()],
    )?;
    Ok(())
}
fn scan(path: &Path, control: &Control, sender: &SyncSender<Event>) -> Result<Stats> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(2))?;
    connection.execute_batch("PRAGMA cache_size=-8192; PRAGMA mmap_size=0; PRAGMA synchronous=NORMAL; PRAGMA wal_autocheckpoint=512;")?;
    let mut stats = Stats::default();
    let start = Instant::now();
    let mut published = Instant::now();
    let mut throttle = Throttle::new();
    while control.checkpoint() {
        let next: Option<(i64, Vec<u8>)> = connection
            .prepare_cached("SELECT id,path FROM directories WHERE pending=1 ORDER BY id LIMIT 1")?
            .query_row([], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((parent, raw_path)) = next else {
            break;
        };
        let directory_path = PathBuf::from(platform::decode(&raw_path));
        let mut reader = match platform::Directory::open(&directory_path) {
            Ok(value) => value,
            Err(error) => {
                let transaction = connection.unchecked_transaction()?;
                issue(&transaction, &mut stats, &directory_path, error)?;
                transaction.execute("UPDATE directories SET pending=0 WHERE id=?1", [parent])?;
                transaction.execute("UPDATE nodes SET status=2 WHERE id=?1", [parent])?;
                transaction.commit()?;
                continue;
            }
        };
        let mut completed = false;
        while !completed && control.checkpoint() {
            let transaction = connection.unchecked_transaction()?;
            let (mut bytes, mut logical, mut files, mut folders) = (0u64, 0u64, 0u64, 0u64);
            for i in 0..1024 {
                if i % 32 == 0 {
                    throttle.yield_if_needed(control);
                    if !control.checkpoint() {
                        break;
                    }
                }
                let entry = match reader.next() {
                    Ok(Some(entry)) => entry,
                    Ok(None) => {
                        completed = true;
                        break;
                    }
                    Err(error) => {
                        issue(&transaction, &mut stats, &directory_path, error)?;
                        // Enumeration failed or the device vanished. Keep committed entries.
                        completed = true;
                        transaction.execute("UPDATE nodes SET status=2 WHERE id=?1", [parent])?;
                        break;
                    }
                };
                let mut allocated = if entry.directory { 0 } else { entry.allocated };
                if !entry.directory {
                    if let Some(identity) = entry.identity {
                        let inserted = transaction
                            .prepare_cached("INSERT OR IGNORE INTO identities VALUES(?1)")?
                            .execute([identity.as_slice()])?;
                        if inserted == 0 {
                            allocated = 0;
                            stats.hardlinks += 1;
                        }
                    }
                    if entry.estimated {
                        stats.estimated += 1;
                    }
                    files += 1;
                    bytes = bytes.saturating_add(allocated);
                    logical = logical.saturating_add(entry.logical);
                } else {
                    folders += 1;
                }
                let status = if entry.directory && entry.link {
                    stats.skipped_links += 1;
                    3
                } else {
                    0
                };
                transaction.prepare_cached("INSERT INTO nodes(parent,name,raw_name,directory,bytes,logical,status) VALUES(?1,?2,?3,?4,?5,?6,?7)")?.execute(
                    params![parent,entry.name.to_string_lossy(),platform::encode(&entry.name),entry.directory,i64::try_from(allocated)?,if entry.directory {0} else {i64::try_from(entry.logical)?},status])?;
                if entry.directory {
                    let path = directory_path.join(&entry.name);
                    transaction
                        .prepare_cached("INSERT INTO directories VALUES(?1,?2,?3)")?
                        .execute(params![
                            transaction.last_insert_rowid(),
                            platform::encode(path.as_os_str()),
                            if entry.link { 0 } else { 1 }
                        ])?;
                }
            }
            aggregate(&transaction, parent, bytes, logical, files, folders)?;
            if completed {
                transaction
                    .prepare_cached("UPDATE directories SET pending=0 WHERE id=?1")?
                    .execute([parent])?;
            }
            transaction.commit()?;
            stats.bytes = stats.bytes.saturating_add(bytes);
            stats.logical = stats.logical.saturating_add(logical);
            stats.files += files;
            stats.folders += folders;
            if published.elapsed() >= Duration::from_millis(300) {
                stats.seconds = start.elapsed().as_secs_f64();
                stats.worker_cpu_seconds = platform::cpu_seconds();
                let _ = sender.try_send(Event::Progress(stats.clone()));
                published = Instant::now();
            }
        }
    }
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_wakes_a_paused_worker() {
        let control = Arc::new(Control::new(Profile::Gentle));
        control.pause(true);
        let worker_control = control.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || sender.send(worker_control.checkpoint()).unwrap());
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        control.stop();
        assert!(!receiver.recv_timeout(Duration::from_secs(1)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn resume_releases_the_paused_worker() {
        let control = Arc::new(Control::new(Profile::Gentle));
        control.pause(true);
        let worker_control = control.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || sender.send(worker_control.checkpoint()).unwrap());
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        control.pause(false);
        assert!(receiver.recv_timeout(Duration::from_secs(1)).unwrap());
        worker.join().unwrap();
    }
}
