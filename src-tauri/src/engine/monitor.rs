//! Thread monitor latar belakang (satu untuk seluruh aplikasi, tick tiap 2 detik).
//!
//! Setiap tick, untuk setiap repo aktif:
//! 1. **Exit detection** — server berhenti sendiri? → status Stopped/Crashed + event.
//! 2. **Readiness**      — port sudah menerima koneksi? → status Running + link localhost.
//! 3. **Metrik**         — CPU & RAM dijumlahkan untuk SELURUH pohon proses (npm → node → esbuild…).
//! 4. **Alert**          — state machine ambang batas:
//!
//! ```text
//!            over threshold                 ≥ sustain_secs
//!   Normal ───────────────▶ Breaching ────────────────────▶ Alerted ──(Ya)──▶ Stop
//!     ▲                        │                              │  │
//!     └──── back to normal ────┘◀──── back to normal ─────────┘  ├─(Tidak)──▶ Snoozed (snooze_secs)
//!           (AlertCleared)                                       └─(tanpa respons ≥ auto_kill_after_secs)──▶ AutoKilled
//! ```
//! 5. **Idle**           — CPU < idle_cpu_percent DAN tanpa output log ≥ 60 dtk, selama
//!                          idle_timeout_mins → dihentikan (IdleStopped).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use super::{
    port,
    state::{Engine, RunningRepo},
};
use crate::engine::types::{EventKind, RepoStatus, AppSettings};

const TICK: Duration = Duration::from_secs(2);
/// Proses non-HTTP (worker/bot) tidak pernah membuka port; setelah ini dianggap Running.
const READY_FALLBACK: Duration = Duration::from_secs(90);
const QUIET_LOG: Duration = Duration::from_secs(60);

enum Action {
    Alert(String),
    Cleared,
    Stop(EventKind, String),
}

pub fn spawn(engine: Arc<Engine>) {
    let _ = thread::Builder::new().name("repolaunch-monitor".into()).spawn(move || {
        let mut sys = System::new();
        let ncpu = thread::available_parallelism().map_or(1, |n| n.get()) as f64;
        loop {
            thread::sleep(TICK);
            tick(&engine, &mut sys, ncpu);
        }
    });
}

fn tick(engine: &Arc<Engine>, sys: &mut System, ncpu: f64) {
    let repos = engine.running_snapshot();
    if repos.is_empty() {
        return;
    }
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    let tree = children_index(sys);
    let settings = engine.settings();

    for repo in repos {
        let serving_since = *repo.serving_since.lock();
        if let Some(since) = serving_since {
            if let Some(code) = poll_exit(&repo) {
                let (status, msg) = match code {
                    Some(0) => (RepoStatus::Stopped, "Proses selesai (exit 0).".to_string()),
                    other => (RepoStatus::Crashed, format!("Proses berhenti tak terduga (exit {other:?}). Lihat log.")),
                };
                if engine.finish(&repo, status, Some(msg.clone())) {
                    engine.emit_repo(EventKind::StatusChanged, &repo, Some(msg));
                }
                continue;
            }
            check_ready(engine, &repo, since);
        }

        let pid = repo.rt.lock().pid;
        let Some(pid) = pid else { continue };
        let (cpu, mem) = tree_usage(sys, &tree, Pid::from_u32(pid), ncpu);
        {
            let mut rt = repo.rt.lock();
            rt.cpu = cpu;
            rt.mem_mb = mem;
        }
        engine.emit_repo(EventKind::Metrics, &repo, None);

        if serving_since.is_none() {
            continue; // jangan peringatkan saat npm install (memang berat & sementara)
        }
        match evaluate(&repo, &settings, cpu, mem) {
            Some(Action::Alert(detail)) => engine.emit_repo(EventKind::ResourceAlert, &repo, Some(detail)),
            Some(Action::Cleared) => engine.emit_repo(EventKind::AlertCleared, &repo, None),
            Some(Action::Stop(kind, msg)) => {
                // Stop bisa memakan waktu (graceful 5 dtk) → jangan blok tick monitor.
                let engine = Arc::clone(engine);
                let id = repo.id.clone();
                let _ = thread::Builder::new()
                    .name("repolaunch-autokill".into())
                    .spawn(move || { let _ = engine.stop(&id, kind, Some(msg)); });
            }
            None => {}
        }
    }
}

/// `Some(exit_code)` jika proses server sudah keluar.
fn poll_exit(repo: &RunningRepo) -> Option<Option<i32>> {
    let mut slot = repo.child.lock();
    let child = slot.as_mut()?;
    match child.try_wait() {
        Ok(Some(status)) => {
            drop(slot.take()); // bersihkan cucu yang mungkin masih hidup
            Some(status.code())
        }
        _ => None,
    }
}

fn check_ready(engine: &Engine, repo: &RunningRepo, since: Instant) {
    let (status, port) = {
        let rt = repo.rt.lock();
        (rt.status, rt.port)
    };
    if status != RepoStatus::Starting {
        return;
    }
    let listening = port::is_listening(port);
    if !listening && since.elapsed() < READY_FALLBACK {
        return;
    }
    {
        let mut rt = repo.rt.lock();
        rt.status = RepoStatus::Running;
        rt.phase = None;
        if listening {
            rt.url.get_or_insert_with(|| format!("http://localhost:{port}"));
        }
    }
    let msg = (!listening).then(|| "Proses berjalan, tetapi tidak membuka port HTTP.".to_string());
    engine.emit_repo(EventKind::StatusChanged, repo, msg);
}

fn children_index(sys: &System) -> HashMap<Pid, Vec<Pid>> {
    let mut map: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, proc_) in sys.processes() {
        if let Some(parent) = proc_.parent() {
            map.entry(parent).or_default().push(*pid);
        }
    }
    map
}

/// (CPU % dinormalisasi 0–100 terhadap total core, RAM MB) untuk seluruh pohon proses.
fn tree_usage(sys: &System, tree: &HashMap<Pid, Vec<Pid>>, root: Pid, ncpu: f64) -> (f64, f64) {
    let (mut cpu, mut mem) = (0.0f64, 0u64);
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(p) = sys.process(pid) {
            cpu += f64::from(p.cpu_usage());
            mem += p.memory();
        }
        if let Some(kids) = tree.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    ((cpu / ncpu).min(100.0), mem as f64 / (1024.0 * 1024.0))
}

fn fmt_mem(mb: f64) -> String {
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.0} MB") }
}

fn evaluate(repo: &RunningRepo, s: &AppSettings, cpu: f64, mem: f64) -> Option<Action> {
    let now = Instant::now();
    let secs = |v: u32| Duration::from_secs(u64::from(v));
    let mut a = repo.alert.lock();

    if s.idle_timeout_mins > 0 {
        let quiet = cpu < s.idle_cpu_percent && repo.last_output.lock().elapsed() >= QUIET_LOG;
        if quiet {
            let since = *a.idle_since.get_or_insert(now);
            if now.duration_since(since) >= secs(s.idle_timeout_mins * 60) {
                a.idle_since = None;
                return Some(Action::Stop(
                    EventKind::IdleStopped,
                    format!(
                        "Repo {} idle selama {} menit dan dihentikan otomatis untuk menghemat resource.",
                        repo.name, s.idle_timeout_mins
                    ),
                ));
            }
        } else {
            a.idle_since = None;
        }
    }

    if a.snoozed_until.is_some_and(|t| now < t) {
        return None;
    }

    let over = cpu > s.cpu_threshold_percent || mem > s.memory_threshold_mb;
    if !over {
        a.breach_since = None;
        return a.alerted_at.take().map(|_| Action::Cleared);
    }

    let since = *a.breach_since.get_or_insert(now);
    match a.alerted_at {
        None if now.duration_since(since) >= secs(s.sustain_secs) => {
            a.alerted_at = Some(now);
            Some(Action::Alert(format!(
                "CPU {cpu:.0}% · RAM {} selama ≥ {} detik",
                fmt_mem(mem),
                s.sustain_secs
            )))
        }
        Some(at) if s.auto_kill_after_secs > 0 && now.duration_since(at) >= secs(s.auto_kill_after_secs) => {
            a.alerted_at = None;
            a.breach_since = None;
            Some(Action::Stop(
                EventKind::AutoKilled,
                format!(
                    "Tidak ada respons dalam {} detik — repo {} dihentikan otomatis (CPU {cpu:.0}%, RAM {}).",
                    s.auto_kill_after_secs,
                    repo.name,
                    fmt_mem(mem)
                ),
            ))
        }
        _ => None,
    }
}
