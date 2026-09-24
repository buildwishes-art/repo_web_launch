//! Engine: state global core RepoLaunch.
//!
//! Model konkurensi:
//! * `running` = map repo aktif. Port yang di-reserve = port semua entri di map ini, dan
//!   alokasi port + insert dilakukan dalam SATU critical section → dua start bersamaan tidak
//!   akan pernah mendapat port yang sama.
//! * Per repo: 1 thread launcher (langkah setup → server), 2 thread pembaca log (stdout/stderr).
//! * 1 thread monitor global (lihat `monitor.rs`).
//! * Urutan lock (untuk mencegah deadlock): registry → running → finished; lock milik
//!   `RunningRepo` selalu diambil sesudahnya dan tidak pernah ditahan saat memanggil Engine.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, bail, Result};
use parking_lot::{Mutex, RwLock};
use regex::Regex;
use uuid::Uuid;

use super::{
    archive, cache,
    scan::{self, ScanOptions},
    detect::{self, LaunchPlan},
    fsutil, git, monitor, port,
    registry::{Registry, RepoRecord},
    sandbox::{self, SandboxPolicy, Sandboxed},
    validation,
};
use crate::engine::types::{
    CoreEvent, EventKind, RepoInfo, RepoStatus, AppSettings, ScanReport, ScanVerdict, SourceKind,
};

/// Penerima event core. Di aplikasi: diteruskan ke frontend (Tauri emit), tray, dan toast.
pub type EventSink = Arc<dyn Fn(&CoreEvent) + Send + Sync>;

const MAX_LOG_LINES: usize = 1000;
const MAX_LINE_CHARS: usize = 2000;
const BYTES_PER_MB: f64 = 1024.0 * 1024.0;

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();

static RE_ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());
/// Hanya URL loopback yang diterima sebagai link — log tidak bisa "menyuntikkan" URL eksternal.
static RE_LOCAL_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):(\d{2,5})(/[A-Za-z0-9/_\-.]*)?").unwrap()
});

pub fn init(data_dir: &Path, sink: EventSink) -> Result<()> {
    if ENGINE.get().is_some() {
        return Ok(());
    }
    let engine = Arc::new(Engine::open(data_dir, sink)?);
    if ENGINE.set(Arc::clone(&engine)).is_ok() {
        monitor::spawn(engine);
    }
    Ok(())
}

pub fn get() -> Result<Arc<Engine>> {
    ENGINE.get().cloned().ok_or_else(|| anyhow!("Core belum diinisialisasi (init belum dipanggil)"))
}

// ───────────────────────────── Tipe runtime ─────────────────────────────

#[derive(Debug, Clone)]
pub struct Runtime {
    pub status: RepoStatus,
    pub phase: Option<String>,
    pub port: u16,
    pub url: Option<String>,
    pub pid: Option<u32>,
    pub cpu: f64,
    pub mem_mb: f64,
}

#[derive(Debug, Default)]
pub struct AlertState {
    pub breach_since: Option<Instant>,
    pub alerted_at: Option<Instant>,
    pub snoozed_until: Option<Instant>,
    pub idle_since: Option<Instant>,
}

pub struct RunningRepo {
    pub id: String,
    pub name: String,
    pub dir: PathBuf,
    pub notice: Option<String>,
    pub cancel: AtomicBool,
    pub child: Mutex<Option<Sandboxed>>,
    pub rt: Mutex<Runtime>,
    pub logs: Mutex<VecDeque<String>>,
    pub last_output: Mutex<Instant>,
    /// `Some` sejak proses server utama di-spawn (bukan langkah setup).
    pub serving_since: Mutex<Option<Instant>>,
    pub alert: Mutex<AlertState>,
}

impl RunningRepo {
    fn new(rec: &RepoRecord, dir: PathBuf, port: u16, notice: Option<String>) -> Self {
        Self {
            id: rec.id.clone(),
            name: rec.name.clone(),
            dir,
            notice,
            cancel: AtomicBool::new(false),
            child: Mutex::new(None),
            rt: Mutex::new(Runtime {
                status: RepoStatus::Starting,
                phase: Some("Menyiapkan".into()),
                port,
                url: None,
                pid: None,
                cpu: 0.0,
                mem_mb: 0.0,
            }),
            logs: Mutex::new(VecDeque::with_capacity(256)),
            last_output: Mutex::new(Instant::now()),
            serving_since: Mutex::new(None),
            alert: Mutex::new(AlertState::default()),
        }
    }

    pub fn push_log(&self, line: String) {
        let mut logs = self.logs.lock();
        if logs.len() >= MAX_LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(line);
    }
}

/// Jejak repo yang sudah berhenti (agar UI tetap bisa menampilkan alasan crash & log terakhir).
struct Finished {
    status: RepoStatus,
    notice: Option<String>,
    logs: Vec<String>,
}

// ───────────────────────────── Engine ─────────────────────────────

pub struct Engine {
    workspace: PathBuf,
    staging: PathBuf,
    tmp_root: PathBuf,
    cache_root: PathBuf,
    settings_file: PathBuf,
    registry: Mutex<Registry>,
    running: Mutex<HashMap<String, Arc<RunningRepo>>>,
    finished: Mutex<HashMap<String, Finished>>,
    settings: RwLock<AppSettings>,
    sink: EventSink,
}

impl Engine {
    fn open(data_dir: &Path, sink: EventSink) -> Result<Self> {
        let workspace = data_dir.join("repos");
        let staging = data_dir.join(".staging");
        let tmp_root = data_dir.join("sandbox-tmp");
        let cache_root = data_dir.join("cache");
        for dir in [&workspace, &staging, &tmp_root, &cache_root] {
            fs::create_dir_all(dir)?;
        }
        // Sisa instalasi yang terputus (crash saat clone/ekstrak).
        cache::clean_dir_contents(&staging);

        let registry = Registry::load(data_dir.join("repos.json"))?;
        let settings_file = data_dir.join("settings.json");
        let settings = fsutil::read_small(&settings_file, 64 * 1024)
            .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
            .map(sanitize_settings)
            .unwrap_or_default();

        Ok(Self {
            workspace,
            staging,
            tmp_root,
            cache_root,
            settings_file,
            registry: Mutex::new(registry),
            running: Mutex::new(HashMap::new()),
            finished: Mutex::new(HashMap::new()),
            settings: RwLock::new(settings),
            sink,
        })
    }

    // ── Event ──

    fn emit(&self, event: CoreEvent) {
        (self.sink)(&event);
    }

    pub(crate) fn emit_repo(&self, kind: EventKind, repo: &RunningRepo, message: Option<String>) {
        let rt = repo.rt.lock().clone();
        self.emit(CoreEvent {
            kind,
            repo_id: repo.id.clone(),
            repo_name: repo.name.clone(),
            status: Some(rt.status),
            port: Some(rt.port),
            url: rt.url,
            cpu_percent: Some(rt.cpu),
            memory_mb: Some(rt.mem_mb),
            message,
        });
    }

    // ── Settings ──

    pub fn settings(&self) -> AppSettings {
        self.settings.read().clone()
    }

    pub fn update_settings(&self, settings: AppSettings) -> Result<AppSettings> {
        let settings = sanitize_settings(settings);
        fsutil::write_atomic(&self.settings_file, &serde_json::to_vec_pretty(&settings)?)?;
        *self.settings.write() = settings.clone();
        Ok(settings)
    }

    // ── Registry ──

    fn record(&self, id: &str) -> Result<RepoRecord> {
        self.registry.lock().get(id).cloned().ok_or_else(|| anyhow!("Repo tidak ditemukan"))
    }

    fn repo_dir(&self, rec: &RepoRecord) -> Result<PathBuf> {
        fsutil::ensure_within(&self.workspace, &self.workspace.join(&rec.dir_name))
    }

    pub fn list(&self) -> Vec<RepoInfo> {
        let records = self.registry.lock().list().to_vec();
        let running = self.running.lock().clone();
        let finished = self.finished.lock();
        records
            .iter()
            .map(|r| to_info(r, running.get(&r.id).map(|a| a.as_ref()), finished.get(&r.id)))
            .collect()
    }

    pub fn add_git(&self, raw_url: &str, name: Option<&str>) -> Result<RepoInfo> {
        let url = validation::validate_git_url(raw_url)?;
        let fallback = validation::name_from_url(&url);
        let name = validation::sanitize_repo_name(pick_name(name, &fallback))?;
        self.install(name, SourceKind::Git, url.to_string(), false, |staging| git::clone_shallow(&url, staging))
    }

    pub fn add_archive(&self, file: &str, name: Option<&str>) -> Result<RepoInfo> {
        let path = PathBuf::from(file);
        let kind = validation::validate_archive(&path)?;
        let fallback = validation::name_from_archive(&path);
        let name = validation::sanitize_repo_name(pick_name(name, &fallback))?;
        let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.install(name, SourceKind::Archive, label, true, |staging| archive::extract(&path, kind, staging))
    }

    /// Semua instalasi dibangun di `.staging/<id>` (karantina), DIPINDAI di sana, lalu di-rename
    /// atomik ke workspace. Workspace tidak pernah berisi repo setengah jadi atau belum dipindai.
    /// Repo yang mencurigakan tetap dipindahkan (agar bisa ditinjau/dihapus dari UI), tetapi
    /// ditandai `Suspicious` dan tidak bisa di-Start tanpa konfirmasi eksplisit.
    fn install(
        &self,
        name: String,
        kind: SourceKind,
        source: String,
        flatten: bool,
        build: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<RepoInfo> {
        let id = Uuid::new_v4().simple().to_string();
        let dir_name = format!("{name}-{}", &id[..8]);
        let staging = self.staging.join(&id);
        let dest = self.workspace.join(&dir_name);

        let opts = ScanOptions { antivirus: self.settings().antivirus_scan, install_time: true };
        let outcome = build(&staging).and_then(|_| {
            let root = archive::content_root(&staging, flatten)?;
            // RepoLaunch khusus proyek web: tolak repo lain sebelum masuk workspace.
            let stack = detect::detect_stack(&root).ok_or_else(|| anyhow!(detect::NOT_WEB_PROJECT))?;
            let report = scan::scan_repo(&root, &opts);
            archive::promote(&staging, &dest, flatten)?;
            Ok((report, stack.label().to_string()))
        });
        let (report, stack) = match outcome {
            Ok(r) => r,
            Err(e) => {
                let _ = fsutil::remove_dir_force(&staging);
                let _ = fsutil::remove_dir_force(&dest);
                return Err(e);
            }
        };

        let record = RepoRecord {
            id,
            name,
            source_kind: kind,
            source,
            dir_name,
            stack,
            created_at: now_secs(),
            scan: Some(report),
        };
        {
            let mut reg = self.registry.lock();
            reg.insert(record.clone());
            reg.save()?;
        }
        Ok(to_info(&record, None, None))
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let rec = self.record(id)?;
        self.stop(id, EventKind::StatusChanged, Some("Dihentikan karena repo dihapus.".into()))?;

        let dir = self.workspace.join(&rec.dir_name);
        if fs::symlink_metadata(&dir).is_ok() {
            let safe = fsutil::ensure_within(&self.workspace, &dir)?;
            fsutil::remove_dir_force(&safe)?;
        }
        let _ = fsutil::remove_dir_force(&self.tmp_root.join(&rec.id));
        self.finished.lock().remove(id);

        let mut reg = self.registry.lock();
        reg.remove(id);
        reg.save()
    }

    // ── Security scan ──

    pub fn scan_report(&self, id: &str) -> Result<Option<ScanReport>> {
        Ok(self.record(id)?.scan)
    }

    /// Scan ulang repo yang sudah terinstal (read-only, tanpa eksekusi) dan simpan hasilnya.
    /// Dependensi yang terinstal (node_modules, .venv) tidak dipindai isinya.
    pub fn rescan(&self, id: &str) -> Result<ScanReport> {
        let rec = self.record(id)?;
        let dir = self.repo_dir(&rec)?;
        let opts = ScanOptions { antivirus: self.settings().antivirus_scan, install_time: false };
        let report = scan::scan_repo(&dir, &opts);
        let mut reg = self.registry.lock();
        if let Some(r) = reg.get_mut(id) {
            r.scan = Some(report.clone());
        }
        reg.save()?;
        Ok(report)
    }

    // ── Proses ──

    /// `allow_suspicious = true` hanya dikirim setelah pengguna mengonfirmasi dialog
    /// "Repo ini terdeteksi tidak normal".
    pub fn start(self: &Arc<Self>, id: &str, allow_suspicious: bool) -> Result<RepoInfo> {
        let mut rec = self.record(id)?;
        let dir = self.repo_dir(&rec)?;
        let settings = self.settings();

        // Repo dari versi lama belum pernah dipindai → pindai dulu sebelum dieksekusi.
        if rec.scan.is_none() && !self.running.lock().contains_key(id) {
            rec.scan = Some(self.rescan(id)?);
        }
        if let Some(report) = rec.scan.as_ref().filter(|r| r.verdict == ScanVerdict::Suspicious) {
            if !allow_suspicious {
                bail!("{}: {}. Tinjau hasil scan sebelum menjalankan.", scan::SUSPICIOUS_NOTE, report.summary);
            }
        }

        let mut running = self.running.lock();
        if let Some(existing) = running.get(id) {
            return Ok(to_info(&rec, Some(existing.as_ref()), None));
        }

        // Smart Port Conflict Resolution (dalam critical section yang sama dengan insert).
        let preferred = detect::default_port(&dir);
        let reserved: HashSet<u16> = running.values().map(|r| r.rt.lock().port).collect();
        let alloc = port::allocate(preferred, &reserved)?;
        let plan = detect::build_plan(&dir, alloc.port, settings.allow_install_scripts)?;

        let notice = match (alloc.relocated, preferred) {
            (true, Some(from)) => {
                let owner = running.values().find(|r| r.rt.lock().port == from).map(|r| r.name.clone());
                Some(match owner {
                    Some(o) => format!("Port {from} sedang dipakai repo '{o}'. Dialihkan otomatis ke port {}.", alloc.port),
                    None => format!("Port {from} sedang dipakai aplikasi lain. Dialihkan otomatis ke port {}.", alloc.port),
                })
            }
            _ => None,
        };

        let mut env = plan.env.clone();
        env.extend(self.sandbox_env(&rec)?);

        let repo = Arc::new(RunningRepo::new(&rec, dir, alloc.port, notice));
        if rec.scan.as_ref().is_some_and(|r| r.verdict == ScanVerdict::Suspicious) {
            repo.push_log(format!("[repolaunch] PERINGATAN: {} — dijalankan atas konfirmasi pengguna.", scan::SUSPICIOUS_NOTE));
        }
        running.insert(rec.id.clone(), Arc::clone(&repo));
        drop(running);
        self.finished.lock().remove(id);

        let engine = Arc::clone(self);
        let launcher_repo = Arc::clone(&repo);
        let spawned = thread::Builder::new()
            .name(format!("launch-{}", rec.name))
            .spawn(move || run_plan(engine, launcher_repo, plan, env));
        if let Err(e) = spawned {
            self.fail(&repo, format!("Gagal membuat thread launcher: {e}"));
            bail!("Gagal memulai repo: {e}");
        }

        self.emit_repo(EventKind::StatusChanged, &repo, repo.notice.clone());
        Ok(to_info(&rec, Some(&*repo), None))
    }

    /// Env tambahan per repo: direktori TEMP privat dan cache paket milik aplikasi
    /// (bukan cache global user), agar "pembersihan cache" tidak menyentuh data lain.
    fn sandbox_env(&self, rec: &RepoRecord) -> Result<Vec<(String, String)>> {
        let tmp = self.tmp_root.join(&rec.id);
        fs::create_dir_all(&tmp)?;
        let tmp = tmp.to_string_lossy().into_owned();
        let npm_cache = self.cache_root.join("npm").to_string_lossy().into_owned();
        let pip_cache = self.cache_root.join("pip").to_string_lossy().into_owned();
        Ok(vec![
            ("TEMP".into(), tmp.clone()),
            ("TMP".into(), tmp.clone()),
            ("TMPDIR".into(), tmp),
            ("npm_config_cache".into(), npm_cache),
            ("npm_config_update_notifier".into(), "false".into()),
            ("npm_config_fund".into(), "false".into()),
            ("PIP_CACHE_DIR".into(), pip_cache),
        ])
    }

    pub fn stop(&self, id: &str, kind: EventKind, message: Option<String>) -> Result<()> {
        let repo = self.running.lock().get(id).cloned();
        let Some(repo) = repo else { return Ok(()) };

        repo.cancel.store(true, Ordering::SeqCst);
        let child = repo.child.lock().take();
        if let Some(mut child) = child {
            child.terminate();
        }
        if self.finish(&repo, RepoStatus::Stopped, message.clone()) {
            if kind != EventKind::StatusChanged {
                self.emit_repo(kind, &repo, message.clone());
            }
            self.emit_repo(EventKind::StatusChanged, &repo, message);
            if self.settings().clean_cache_on_stop {
                let _ = cache::clean_repo(&repo.dir);
                cache::clean_dir_contents(&self.tmp_root.join(id));
            }
        }
        Ok(())
    }

    /// Mengeluarkan repo dari `running` (sekali saja — `false` jika sudah dikeluarkan pihak lain),
    /// mematikan sisa pohon prosesnya, dan menyimpan jejak untuk UI.
    pub(crate) fn finish(&self, repo: &Arc<RunningRepo>, status: RepoStatus, message: Option<String>) -> bool {
        let removed = {
            let mut map = self.running.lock();
            match map.get(&repo.id) {
                Some(current) if Arc::ptr_eq(current, repo) => map.remove(&repo.id),
                _ => None,
            }
        };
        if removed.is_none() {
            return false;
        }
        repo.cancel.store(true, Ordering::SeqCst);
        drop(repo.child.lock().take()); // Drop ⇒ kill tree

        {
            let mut rt = repo.rt.lock();
            rt.status = status;
            rt.phase = None;
            rt.pid = None;
            rt.url = None;
            rt.cpu = 0.0;
            rt.mem_mb = 0.0;
        }
        if let Some(m) = &message {
            repo.push_log(format!("[repolaunch] {m}"));
        }
        let logs = repo.logs.lock().iter().cloned().collect();
        let notice = if status == RepoStatus::Crashed { message } else { None };
        self.finished.lock().insert(repo.id.clone(), Finished { status, notice, logs });
        true
    }

    pub(crate) fn fail(&self, repo: &Arc<RunningRepo>, message: String) {
        if self.finish(repo, RepoStatus::Crashed, Some(message.clone())) {
            self.emit_repo(EventKind::StatusChanged, repo, Some(message));
        }
    }

    pub(crate) fn running_snapshot(&self) -> Vec<Arc<RunningRepo>> {
        self.running.lock().values().cloned().collect()
    }

    /// Jumlah repo aktif (untuk tooltip tray).
    pub fn running_count(&self) -> usize {
        self.running.lock().len()
    }

    /// Menghentikan semua repo (menu tray "Hentikan semua" / saat keluar aplikasi).
    pub fn stop_all(&self, reason: &str) {
        let ids: Vec<String> = self.running.lock().keys().cloned().collect();
        for id in ids {
            let _ = self.stop(&id, EventKind::StatusChanged, Some(reason.to_string()));
        }
    }

    pub fn logs(&self, id: &str) -> Vec<String> {
        if let Some(repo) = self.running.lock().get(id).cloned() {
            return repo.logs.lock().iter().cloned().collect();
        }
        self.finished.lock().get(id).map(|f| f.logs.clone()).unwrap_or_default()
    }

    pub fn respond_alert(&self, id: &str, kill: bool) -> Result<()> {
        if kill {
            return self.stop(id, EventKind::StatusChanged, Some("Dihentikan dari notifikasi resource.".into()));
        }
        let repo = self.running.lock().get(id).cloned();
        if let Some(repo) = repo {
            let snooze = Duration::from_secs(u64::from(self.settings().snooze_secs));
            let mut alert = repo.alert.lock();
            alert.alerted_at = None;
            alert.breach_since = None;
            alert.snoozed_until = Some(Instant::now() + snooze);
        }
        Ok(())
    }

    pub fn clean_cache(&self, id: Option<&str>) -> Result<f64> {
        let mut freed = 0u64;
        match id {
            Some(id) => {
                if self.running.lock().contains_key(id) {
                    bail!("Hentikan repo terlebih dahulu sebelum membersihkan cache-nya");
                }
                let rec = self.record(id)?;
                freed += cache::clean_repo(&self.repo_dir(&rec)?)?;
                freed += cache::clean_dir_contents(&self.tmp_root.join(id));
            }
            None => {
                let records = self.registry.lock().list().to_vec();
                let running: HashSet<String> = self.running.lock().keys().cloned().collect();
                for rec in records.iter().filter(|r| !running.contains(&r.id)) {
                    if let Ok(dir) = self.repo_dir(rec) {
                        freed += cache::clean_repo(&dir).unwrap_or(0);
                    }
                    freed += cache::clean_dir_contents(&self.tmp_root.join(&rec.id));
                }
                // Cache npm/pip bersama hanya dibersihkan saat tidak ada repo yang berjalan.
                if running.is_empty() {
                    freed += cache::clean_dir_contents(&self.cache_root);
                }
            }
        }
        Ok(freed as f64 / BYTES_PER_MB)
    }
}

// ───────────────────────────── Launcher & log ─────────────────────────────

/// Menjalankan langkah-langkah plan secara berurutan di thread launcher.
fn run_plan(engine: Arc<Engine>, repo: Arc<RunningRepo>, plan: LaunchPlan, env: Vec<(String, String)>) {
    let policy = SandboxPolicy::from(&engine.settings());
    repo.push_log(format!("[repolaunch] Stack: {} · port {}", plan.stack.label(), repo.rt.lock().port));

    for step in &plan.steps {
        if repo.cancel.load(Ordering::SeqCst) {
            return;
        }
        {
            let mut rt = repo.rt.lock();
            rt.status = if step.long_running { RepoStatus::Starting } else { RepoStatus::Installing };
            rt.phase = Some(step.label.clone());
        }
        engine.emit_repo(EventKind::StatusChanged, &repo, None);
        repo.push_log(format!("$ {} {}", step.program, step.args.join(" ")));

        let mut child = match sandbox::spawn(step, &repo.dir, &env, &policy) {
            Ok(c) => c,
            Err(e) => {
                engine.fail(&repo, format!("{}: {e:#}", step.label));
                return;
            }
        };
        if let Some(out) = child.take_stdout() {
            spawn_log_reader(Arc::clone(&engine), Arc::clone(&repo), out);
        }
        if let Some(err) = child.take_stderr() {
            spawn_log_reader(Arc::clone(&engine), Arc::clone(&repo), err);
        }
        repo.rt.lock().pid = Some(child.pid());
        {
            let mut slot = repo.child.lock();
            // stop() men-set `cancel` SEBELUM mengambil lock ini, jadi pemeriksaan di bawah
            // menutup race "proses di-spawn tepat setelah stop" (tidak ada proses yatim).
            if repo.cancel.load(Ordering::SeqCst) {
                return; // `child` di-drop ⇒ pohon proses dimatikan
            }
            *slot = Some(child);
        }

        if step.long_running {
            *repo.serving_since.lock() = Some(Instant::now());
            return; // monitor mengambil alih: readiness, exit, metrik
        }

        loop {
            if repo.cancel.load(Ordering::SeqCst) {
                return;
            }
            let polled = {
                let mut slot = repo.child.lock();
                match slot.as_mut() {
                    Some(c) => c.try_wait(),
                    None => return,
                }
            };
            match polled {
                Ok(Some(status)) => {
                    drop(repo.child.lock().take());
                    repo.rt.lock().pid = None;
                    if !status.success() {
                        engine.fail(&repo, format!("Langkah '{}' gagal ({status}). Lihat log.", step.label));
                        return;
                    }
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(250)),
                Err(e) => {
                    engine.fail(&repo, format!("Gagal memantau proses: {e}"));
                    return;
                }
            }
        }
    }
}

fn spawn_log_reader<R: Read + Send + 'static>(engine: Arc<Engine>, repo: Arc<RunningRepo>, stream: R) {
    let _ = thread::Builder::new().name(format!("log-{}", repo.name)).spawn(move || {
        for chunk in BufReader::new(stream).split(b'\n') {
            let Ok(bytes) = chunk else { break };
            let raw = String::from_utf8_lossy(&bytes);
            let line: String = RE_ANSI
                .replace_all(raw.trim_end_matches('\r'), "")
                .chars()
                .take(MAX_LINE_CHARS)
                .collect();
            *repo.last_output.lock() = Instant::now();
            detect_url(&engine, &repo, &line);
            repo.push_log(line);
        }
    });
}

/// Jika aplikasi mencetak URL localhost (mis. "Local: http://localhost:5174/"), pakai itu sebagai
/// link — menangani repo yang mengabaikan port yang kita berikan.
fn detect_url(engine: &Engine, repo: &RunningRepo, line: &str) {
    let Some(caps) = RE_LOCAL_URL.captures(line) else { return };
    let Ok(port) = caps[1].parse::<u16>() else { return };
    let path = caps.get(2).map_or("", |m| m.as_str());
    let message = {
        let mut rt = repo.rt.lock();
        if rt.url.is_some() || rt.status == RepoStatus::Running {
            return;
        }
        let msg = (port != rt.port)
            .then(|| format!("Repo tidak memakai port {} yang dialokasikan; terdeteksi di port {port}.", rt.port));
        rt.port = port;
        rt.url = Some(format!("http://localhost:{port}{path}"));
        msg
    };
    engine.emit_repo(EventKind::UrlDetected, repo, message);
}

// ───────────────────────────── Helper ─────────────────────────────

fn to_info(rec: &RepoRecord, running: Option<&RunningRepo>, finished: Option<&Finished>) -> RepoInfo {
    let mut info = RepoInfo {
        id: rec.id.clone(),
        name: rec.name.clone(),
        source_kind: rec.source_kind,
        source: rec.source.clone(),
        stack: rec.stack.clone(),
        created_at: rec.created_at,
        status: RepoStatus::Stopped,
        port: None,
        url: None,
        pid: None,
        cpu_percent: 0.0,
        memory_mb: 0.0,
        notice: None,
        phase: None,
        scan_verdict: rec.scan.as_ref().map_or(ScanVerdict::NotScanned, |s| s.verdict),
        scan_summary: rec.scan.as_ref().map(|s| s.summary.clone()),
    };
    if let Some(r) = running {
        let rt = r.rt.lock();
        info.status = rt.status;
        info.port = Some(rt.port);
        info.url = rt.url.clone();
        info.pid = rt.pid;
        info.cpu_percent = rt.cpu;
        info.memory_mb = rt.mem_mb;
        info.phase = rt.phase.clone();
        info.notice = r.notice.clone();
    } else if let Some(f) = finished {
        info.status = f.status;
        info.notice = f.notice.clone();
    }
    info
}

fn pick_name<'a>(given: Option<&'a str>, fallback: &'a str) -> &'a str {
    given.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(fallback)
}

fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Clamp semua nilai ke rentang yang masuk akal (input berasal dari UI / file yang bisa diedit).
fn sanitize_settings(mut s: AppSettings) -> AppSettings {
    s.cpu_threshold_percent = s.cpu_threshold_percent.clamp(5.0, 100.0);
    s.memory_threshold_mb = s.memory_threshold_mb.clamp(64.0, 1024.0 * 1024.0);
    s.sustain_secs = s.sustain_secs.clamp(5, 3600);
    s.auto_kill_after_secs = s.auto_kill_after_secs.min(3600);
    s.snooze_secs = s.snooze_secs.clamp(30, 24 * 3600);
    s.idle_timeout_mins = s.idle_timeout_mins.min(24 * 60);
    s.idle_cpu_percent = s.idle_cpu_percent.clamp(0.0, 50.0);
    s.cpu_rate_limit_percent = s.cpu_rate_limit_percent.min(100);
    if s.memory_hard_limit_mb != 0 {
        s.memory_hard_limit_mb = s.memory_hard_limit_mb.max(128);
    }
    s
}
