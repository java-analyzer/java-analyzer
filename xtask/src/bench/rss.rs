//! Portable process-tree RSS memory parser and background sampler.

use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// Interval between background RSS polls.
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Peak and steady-state RSS summary in MiB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RssSummary {
    pub peak_rss_mb: f64,
    pub steady_rss_mb: f64,
}

/// Parses `ps -axo pid=,ppid=,rss=` output and sums RSS (in KiB) for `root_pid` and all descendants.
///
/// Malformed lines are skipped, and cycles in parent pointers are guarded against via a visited set.
pub fn parse_ps_tree_rss_kb(ps_output: &str, root_pid: u32) -> u64 {
    let mut rss_by_pid: BTreeMap<u32, u64> = BTreeMap::new();
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();

    for line in ps_output.lines() {
        let mut parts = line.split_whitespace();
        let (Some(pid_s), Some(ppid_s), Some(rss_s)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let (Ok(pid), Ok(ppid), Ok(rss_kb)) = (
            pid_s.parse::<u32>(),
            ppid_s.parse::<u32>(),
            rss_s.parse::<u64>(),
        ) else {
            continue;
        };
        rss_by_pid.insert(pid, rss_kb);
        children.entry(ppid).or_default().push(pid);
    }

    if !rss_by_pid.contains_key(&root_pid) {
        return 0;
    }

    let mut total_kb = 0u64;
    let mut visited = BTreeSet::new();
    let mut stack = vec![root_pid];

    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }
        if let Some(&kb) = rss_by_pid.get(&pid) {
            total_kb = total_kb.saturating_add(kb);
        }
        if let Some(kids) = children.get(&pid) {
            for &kid in kids {
                if !visited.contains(&kid) {
                    stack.push(kid);
                }
            }
        }
    }

    total_kb
}

/// Samples current process-tree RSS in KiB using `ps -axo pid=,ppid=,rss=`.
pub fn sample_process_tree_rss_kb(root_pid: u32) -> u64 {
    let Ok(output) = Command::new("ps")
        .args(["-axo", "pid=,ppid=,rss="])
        .output()
    else {
        return 0;
    };
    if !output.status.success() {
        return 0;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_ps_tree_rss_kb(&text, root_pid)
}

/// Lists all descendant PIDs of `root_pid` (excluding `root_pid` itself) from `ps` output.
pub fn parse_ps_descendants(ps_output: &str, root_pid: u32) -> Vec<u32> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for line in ps_output.lines() {
        let mut parts = line.split_whitespace();
        let (Some(pid_s), Some(ppid_s)) = (parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(pid), Ok(ppid)) = (pid_s.parse::<u32>(), ppid_s.parse::<u32>()) else {
            continue;
        };
        children.entry(ppid).or_default().push(pid);
    }

    let mut visited = BTreeSet::new();
    let mut result = Vec::new();
    let mut stack = vec![root_pid];
    visited.insert(root_pid);

    while let Some(pid) = stack.pop() {
        if let Some(kids) = children.get(&pid) {
            for &kid in kids {
                if visited.insert(kid) {
                    result.push(kid);
                    stack.push(kid);
                }
            }
        }
    }
    result
}

#[derive(Default)]
struct RssBuffer {
    samples_kb: Vec<u64>,
    steady_start_idx: Option<usize>,
}

/// Background RSS polling sampler for a server PID and all descendant processes.
pub struct RssSampler {
    root_pid: u32,
    stop: Arc<AtomicBool>,
    buffer: Arc<Mutex<RssBuffer>>,
    handle: Option<JoinHandle<()>>,
}

impl RssSampler {
    /// Spawns a 250 ms polling thread tracking `root_pid`'s full process tree RSS.
    pub fn start(root_pid: u32) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let buffer = Arc::new(Mutex::new(RssBuffer::default()));

        let initial_kb = sample_process_tree_rss_kb(root_pid);
        if initial_kb > 0
            && let Ok(mut guard) = buffer.lock()
        {
            guard.samples_kb.push(initial_kb);
        }

        let stop_bg = Arc::clone(&stop);
        let buf_bg = Arc::clone(&buffer);
        let handle = thread::spawn(move || {
            while !stop_bg.load(Ordering::Relaxed) {
                thread::sleep(POLL_INTERVAL);
                if stop_bg.load(Ordering::Relaxed) {
                    break;
                }
                let kb = sample_process_tree_rss_kb(root_pid);
                if kb > 0
                    && let Ok(mut guard) = buf_bg.lock()
                {
                    guard.samples_kb.push(kb);
                }
            }
        });

        Self {
            root_pid,
            stop,
            buffer,
            handle: Some(handle),
        }
    }

    /// Marks the beginning of the steady-state measurement phase.
    pub fn mark_steady_start(&self) {
        let kb = sample_process_tree_rss_kb(self.root_pid);
        if let Ok(mut guard) = self.buffer.lock() {
            if kb > 0 {
                guard.samples_kb.push(kb);
            }
            guard.steady_start_idx = Some(guard.samples_kb.len());
        }
    }

    /// Stops polling, collects a final sample if needed, and returns peak/steady MiB metrics.
    pub fn finish(mut self) -> RssSummary {
        self.stop_thread();
        let final_kb = sample_process_tree_rss_kb(self.root_pid);
        let guard = match self.buffer.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        let mut all = guard.samples_kb.clone();
        if all.is_empty() && final_kb > 0 {
            all.push(final_kb);
        }

        let peak_kb = all.iter().copied().max().unwrap_or(0);
        let steady_slice = match guard.steady_start_idx {
            Some(idx) if idx < all.len() => all.get(idx..).unwrap_or(&all),
            _ => all.as_slice(),
        };
        let steady_kb = if steady_slice.is_empty() {
            peak_kb as f64
        } else {
            let sum: u64 = steady_slice.iter().copied().sum();
            sum as f64 / steady_slice.len() as f64
        };

        RssSummary {
            peak_rss_mb: peak_kb as f64 / 1024.0,
            steady_rss_mb: steady_kb / 1024.0,
        }
    }

    fn stop_thread(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RssSampler {
    fn drop(&mut self) {
        self.stop_thread();
    }
}
