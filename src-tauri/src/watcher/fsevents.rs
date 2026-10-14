//! macOS: one FSEvents stream over every folder an open file sits in.
//!
//! `notify`'s FSEvents backend stops, joins and restarts its stream on every
//! `watch` and `unwatch`, on the calling thread, which is the main thread
//! under the registry's lock, and the new stream starts from "now": a change
//! landing between the stop and the start is never reported, and the tab shows
//! stale text with no prompt (TS-18).
//!
//! Here a thread of its own owns the streams. It folds a burst of requests
//! into one, drops every folder another one in the set already covers, and
//! starts a single stream over what is left before it lets the old one go, so
//! the two overlap rather than leave a gap. The old stream keeps reporting for
//! [`OVERLAP`] after the new one is running, which covers anything the daemon
//! had not yet handed it, and is stopped on a thread of its own. A change both
//! streams saw arrives twice and costs the watcher one `stat`
//! ([`writ_core::watcher::sighting`]).
//!
//! `watch` waits for the stream that includes the new folder to be running,
//! and nothing else: the caller never stops or joins a stream. That wait is
//! what lets a caller rely on hearing every change after `watch` returns, as
//! it could with `notify`.
//!
//! The new stream starts from "now" rather than from the last event id the old
//! one delivered. A stream started from a past id replays the daemon's history
//! before it reports anything live, and that replay took from 0.6 s to 3.4 s
//! here, holding back every change made after the open for as long.
//!
//! The stream sees whole trees; only an entry directly inside a watched folder,
//! or the folder itself, is passed on, which is the non-recursive watch every
//! other backend gives.

use std::collections::{BTreeSet, HashMap};
use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use fsevent_sys as fs;
use fsevent_sys::core_foundation as cf;
use notify::{Config, Event, EventHandler, EventKind, RecursiveMode, Watcher, WatcherKind};

use crate::poison::recover_poison;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopIsWaiting(run_loop: cf::CFRunLoopRef) -> cf::Boolean;
}

/// How long a replaced stream keeps reporting after its successor started.
pub const OVERLAP: Duration = Duration::from_secs(1);

/// The longest `watch` waits for the stream that includes its folder. Starting
/// a stream is an exchange with `fseventsd`: a few milliseconds on an idle
/// machine, and over half a second measured here under a full build.
const START_WAIT: Duration = Duration::from_secs(10);

/// The handler every stream reports into, shared across rebuilds.
type SharedHandler = Arc<Mutex<dyn EventHandler>>;

/// The folders `folders` names, less every one that sits inside another of
/// them: a stream over a folder already sees everything under it.
pub fn stream_roots(folders: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    // In path order a folder comes before everything inside it.
    for folder in folders {
        if !roots.iter().any(|root| folder.starts_with(root)) {
            roots.push(folder.clone());
        }
    }
    roots
}

/// Whether an event at `path` belongs to a folder in `folders`: the folder
/// itself, or an entry directly inside it.
pub fn is_reported(path: &Path, folders: &BTreeSet<PathBuf>) -> bool {
    folders.contains(path) || path.parent().is_some_and(|parent| folders.contains(parent))
}

/// A [`Watcher`] over FSEvents whose caller never stops or joins a stream.
pub struct FsEventsWatcher {
    /// Each folder as the caller named it, and as the stream sees it.
    folders: HashMap<PathBuf, PathBuf>,
    requests: mpsc::Sender<Request>,
}

/// The folders to watch, and who is waiting for them to be watched.
struct Request {
    folders: BTreeSet<PathBuf>,
    started: Option<mpsc::Sender<()>>,
}

impl FsEventsWatcher {
    fn request(&self, started: Option<mpsc::Sender<()>>) {
        let _ = self.requests.send(Request {
            folders: self.folders.values().cloned().collect(),
            started,
        });
    }
}

impl Watcher for FsEventsWatcher {
    fn new<F: EventHandler>(event_handler: F, _config: Config) -> notify::Result<Self> {
        let handler: SharedHandler = Arc::new(Mutex::new(event_handler));
        let (requests, received) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("writ fsevents".to_string())
            .spawn(move || rebuild_streams(received, handler))
            .map_err(notify::Error::io)?;
        Ok(Self {
            folders: HashMap::new(),
            requests,
        })
    }

    fn watch(&mut self, path: &Path, _recursive_mode: RecursiveMode) -> notify::Result<()> {
        let resolved = path
            .canonicalize()
            .map_err(|e| notify::Error::io(e).add_path(path.to_path_buf()))?;
        if !resolved.is_dir() {
            return Err(notify::Error::path_not_found().add_path(path.to_path_buf()));
        }
        if self.folders.insert(path.to_path_buf(), resolved).is_none() {
            let (started, running) = mpsc::channel();
            self.request(Some(started));
            let _ = running.recv_timeout(START_WAIT);
        }
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        if self.folders.remove(path).is_none() {
            return Err(notify::Error::watch_not_found().add_path(path.to_path_buf()));
        }
        // A folder left in the running stream only reports a little more
        // than asked for until the rebuild, so nothing waits on it.
        self.request(None);
        Ok(())
    }

    fn kind() -> WatcherKind {
        WatcherKind::Fsevent
    }
}

/// The stream thread's loop: one stream at a time, replaced whenever the set
/// of folders changes, until the watcher is dropped.
fn rebuild_streams(requests: mpsc::Receiver<Request>, handler: SharedHandler) {
    let mut running: Option<RunningStream> = None;
    while let Ok(first) = requests.recv() {
        // A burst of opens is one rebuild, for the newest set.
        let mut folders = first.folders;
        let mut waiting: Vec<mpsc::Sender<()>> = first.started.into_iter().collect();
        while let Ok(newer) = requests.try_recv() {
            folders = newer.folders;
            waiting.extend(newer.started);
        }
        let roots = stream_roots(&folders);
        let next = if roots.is_empty() {
            None
        } else {
            match RunningStream::start(&roots, folders, handler.clone()) {
                Ok(stream) => Some(stream),
                Err(message) => {
                    tracing::error!(%message, "the open files' folders could not be watched");
                    None
                }
            }
        };
        for started in waiting {
            let _ = started.send(());
        }
        if let Some(old) = std::mem::replace(&mut running, next) {
            let _ = std::thread::Builder::new()
                .name("writ fsevents retire".to_string())
                .spawn(move || {
                    std::thread::sleep(OVERLAP);
                    old.stop();
                });
        }
    }
    if let Some(old) = running.take() {
        old.stop();
    }
}

/// What the stream's callback reads.
struct StreamContext {
    folders: BTreeSet<PathBuf>,
    handler: SharedHandler,
}

/// A run loop pointer, moved to the thread that stops it.
struct RunLoop(cf::CFRunLoopRef);

// SAFETY: a CFRunLoop may be stopped from another thread; that is the one
// thing done with this pointer off its own thread.
unsafe impl Send for RunLoop {}

/// One stream, scheduled on a run loop thread of its own.
struct RunningStream {
    run_loop: RunLoop,
    thread: JoinHandle<()>,
}

impl RunningStream {
    fn start(
        roots: &[PathBuf],
        folders: BTreeSet<PathBuf>,
        handler: SharedHandler,
    ) -> Result<Self, String> {
        let roots: Vec<CString> = roots
            .iter()
            .filter_map(|root| root.to_str())
            .filter_map(|root| CString::new(root).ok())
            .collect();
        if roots.is_empty() {
            return Err("no folder has a path FSEvents can take".to_string());
        }
        let context = Box::new(StreamContext { folders, handler });
        let (ready, started) = mpsc::channel::<Result<RunLoop, String>>();
        let thread = std::thread::Builder::new()
            .name("writ fsevents stream".to_string())
            .spawn(move || run_stream(roots, context, ready))
            .map_err(|e| e.to_string())?;
        match started.recv() {
            Ok(Ok(run_loop)) => Ok(Self { run_loop, thread }),
            Ok(Err(message)) => {
                let _ = thread.join();
                Err(message)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the stream thread ended before it started".to_string())
            }
        }
    }

    fn stop(self) {
        // SAFETY: the run loop belongs to a thread that is alive until it is
        // joined below. `CFRunLoopStop` before the loop runs would be lost, so
        // it waits for the loop to be waiting first, as `notify` does.
        unsafe {
            while CFRunLoopIsWaiting(self.run_loop.0) == 0 {
                std::thread::yield_now();
            }
            cf::CFRunLoopStop(self.run_loop.0);
        }
        let _ = self.thread.join();
    }
}

/// Creates, runs and tears down one stream, all on this thread.
fn run_stream(
    roots: Vec<CString>,
    context: Box<StreamContext>,
    ready: mpsc::Sender<Result<RunLoop, String>>,
) {
    let info = Box::into_raw(context);
    // SAFETY: every CoreFoundation object created here is released here;
    // `info` outlives the stream, which is invalidated and released before
    // the box is taken back.
    unsafe {
        let paths =
            cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks);
        for root in &roots {
            let path = cf::CFStringCreateWithCString(
                cf::kCFAllocatorDefault,
                root.as_ptr(),
                cf::kCFStringEncodingUTF8,
            );
            if !path.is_null() {
                cf::CFArrayAppendValue(paths, path);
                cf::CFRelease(path);
            }
        }
        let stream_context = fs::FSEventStreamContext {
            version: 0,
            info: info.cast::<c_void>(),
            retain: None,
            release: None,
            copy_description: None,
        };
        let stream = fs::FSEventStreamCreate(
            cf::kCFAllocatorDefault,
            callback,
            &stream_context,
            paths,
            fs::kFSEventStreamEventIdSinceNow,
            0.0,
            fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
        );
        cf::CFRelease(paths);
        if stream.is_null() {
            drop(Box::from_raw(info));
            let _ = ready.send(Err("FSEventStreamCreate failed".to_string()));
            return;
        }
        let run_loop = cf::CFRunLoopGetCurrent();
        fs::FSEventStreamScheduleWithRunLoop(stream, run_loop, cf::kCFRunLoopDefaultMode);
        if fs::FSEventStreamStart(stream) == 0 {
            fs::FSEventStreamInvalidate(stream);
            fs::FSEventStreamRelease(stream);
            drop(Box::from_raw(info));
            let _ = ready.send(Err("FSEventStreamStart failed".to_string()));
            return;
        }
        let _ = ready.send(Ok(RunLoop(run_loop)));
        cf::CFRunLoopRun();
        fs::FSEventStreamStop(stream);
        fs::FSEventStreamInvalidate(stream);
        fs::FSEventStreamRelease(stream);
        drop(Box::from_raw(info));
    }
}

extern "C" fn callback(
    _stream: fs::FSEventStreamRef,
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const fs::FSEventStreamEventFlags,
    _ids: *const fs::FSEventStreamEventId,
) {
    // A panic must not cross into CoreServices.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: FSEvents hands `count` entries in each array, C strings in
        // `paths` (no `UseCFTypes` flag), and `info` is the context
        // `run_stream` keeps alive for the life of the stream.
        unsafe { deliver(info, count, paths, flags) }
    }));
}

/// # Safety
///
/// The arguments are the ones FSEvents passed [`callback`].
unsafe fn deliver(
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const fs::FSEventStreamEventFlags,
) {
    // SAFETY: see the function's contract.
    let context = unsafe { &*info.cast::<StreamContext>() };
    let paths = paths.cast::<*const c_char>();
    let mut reported = Vec::new();
    for i in 0..count {
        // SAFETY: `i < count`, the length of both arrays.
        let (flag, raw) = unsafe { (*flags.add(i), *paths.add(i)) };
        if flag & fs::kFSEventStreamEventFlagHistoryDone != 0 {
            continue;
        }
        // SAFETY: FSEvents passes NUL-terminated paths.
        let Ok(path) = unsafe { CStr::from_ptr(raw) }.to_str() else {
            continue;
        };
        let path = PathBuf::from(path);
        if is_reported(&path, &context.folders) {
            reported.push(path);
        }
    }
    if reported.is_empty() {
        return;
    }
    let mut handler = recover_poison(context.handler.lock(), "watcher::fsevents::deliver");
    for path in reported {
        handler.handle_event(Ok(Event::new(EventKind::Any).add_path(path)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> BTreeSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_folder_inside_another_needs_no_stream_of_its_own() {
        let folders = set(&["/a", "/a/b", "/a/b/c", "/ab", "/z"]);
        assert_eq!(
            stream_roots(&folders),
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/ab"),
                PathBuf::from("/z")
            ]
        );
    }

    #[test]
    fn only_an_entry_directly_inside_a_watched_folder_is_reported() {
        let folders = set(&["/a", "/a/b/c"]);
        assert!(is_reported(Path::new("/a/note.md"), &folders));
        assert!(is_reported(Path::new("/a"), &folders));
        assert!(is_reported(Path::new("/a/b/c/x.md"), &folders));
        assert!(!is_reported(Path::new("/a/b/note.md"), &folders));
        assert!(!is_reported(Path::new("/elsewhere/x.md"), &folders));
    }

    fn wait_for(rx: &mpsc::Receiver<notify::Result<Event>>, path: &Path) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(Ok(event)) if event.paths.iter().any(|p| p == path) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    #[test]
    fn a_write_right_after_watch_returns_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = FsEventsWatcher::new(tx, Config::default()).unwrap();

        watcher.watch(&root, RecursiveMode::NonRecursive).unwrap();
        let file = root.join("a.md");
        std::fs::write(&file, "x").unwrap();
        assert!(wait_for(&rx, &file), "the write was never reported");
    }

    #[test]
    fn a_write_landing_while_the_stream_is_rebuilt_is_still_reported() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_root = first.path().canonicalize().unwrap();
        let second_root = second.path().canonicalize().unwrap();
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let watcher = Arc::new(Mutex::new(
            FsEventsWatcher::new(tx, Config::default()).unwrap(),
        ));
        watcher
            .lock()
            .unwrap()
            .watch(&first_root, RecursiveMode::NonRecursive)
            .unwrap();

        // Adding a folder replaces the stream; the write races the swap.
        let adder = {
            let watcher = watcher.clone();
            let second_root = second_root.clone();
            std::thread::spawn(move || {
                watcher
                    .lock()
                    .unwrap()
                    .watch(&second_root, RecursiveMode::NonRecursive)
                    .unwrap();
            })
        };
        let file = first_root.join("during.md");
        std::fs::write(&file, "x").unwrap();
        adder.join().unwrap();
        assert!(wait_for(&rx, &file), "a write during the rebuild was lost");

        let later = second_root.join("after.md");
        std::fs::write(&later, "y").unwrap();
        assert!(wait_for(&rx, &later), "the added folder is not watched");
    }

    #[test]
    fn an_unwatched_folder_is_refused_a_second_time() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = FsEventsWatcher::new(tx, Config::default()).unwrap();
        watcher
            .watch(dir.path(), RecursiveMode::NonRecursive)
            .unwrap();
        watcher.unwatch(dir.path()).unwrap();
        assert!(watcher.unwatch(dir.path()).is_err());
        assert!(watcher
            .watch(&dir.path().join("missing"), RecursiveMode::NonRecursive)
            .is_err());
    }
}
