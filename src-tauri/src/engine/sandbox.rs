//! Spawn proses terisolasi + kontrol pohon proses.
//!
//! Lapisan isolasi yang diterapkan ke SETIAP proses repo:
//!
//! | Lapisan              | Implementasi                                                   |
//! |----------------------|----------------------------------------------------------------|
//! | Tanpa shell          | `Command` + args vektor (std meng-escape `.cmd` dengan aman)   |
//! | Allowlist program    | npm/pnpm/yarn/bun/node/python/php/composer                     |
//! | Env dibersihkan      | `env_clear()` + allowlist + TEMP privat                        |
//! | CWD terkunci         | direktori repo (sudah dikanonikalisasi)                        |
//! | Grup proses          | Job Object (KILL_ON_JOB_CLOSE)                                 |
//! | Batas CPU            | affinity + CPU rate hard cap + prioritas BELOW_NORMAL          |
//! | Batas RAM            | Job memory limit (opsional) + monitor (alert/kill)             |
//! | Tanpa jendela konsol | CREATE_NO_WINDOW                                               |
//!
//! RAII: `Sandboxed` di-drop ⇒ seluruh pohon proses dimatikan. Bahkan jika RepoLaunch crash,
//! Windows menutup handle Job ⇒ semua proses repo ikut mati (tidak ada proses yatim).
//!
//! Catatan jujur: ini *containment*, bukan sandbox keamanan penuh. Kode repo tetap berjalan
//! dengan hak user yang sama. Untuk repo yang tidak dipercaya, gunakan mode kontainer
//! (Docker/WSL/Windows Sandbox) — lihat docs/ARCHITECTURE.md.

use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
};

use anyhow::{bail, Context, Result};

use super::{detect::Step, fsutil};
use crate::engine::types::AppSettings;

/// Runtime web yang boleh dijalankan (di-resolve lewat PATH).
pub const ALLOWED_TOOLS: &[&str] = &["npm", "pnpm", "yarn", "bun", "node", "python", "php", "composer"];

/// Nama program khusus: RepoLaunch.exe sendiri dalam mode server statis
/// (`RepoLaunch.exe --serve-static <dir> <port>`), dijalankan sebagai proses anak di Job Object
/// seperti runtime lain sehingga ikut dimonitor & dibatasi resource-nya.
pub const SELF_STATIC_SERVER: &str = "@repolaunch-static";
pub const STATIC_SERVER_FLAG: &str = "--serve-static";

/// Variabel env host yang boleh diteruskan. Semua lainnya (token, AWS_*, GITHUB_TOKEN, dst) dibuang.
const ENV_ALLOWLIST: &[&str] = &[
    "PATH", "PATHEXT", "SYSTEMROOT", "WINDIR", "COMSPEC", "SYSTEMDRIVE", "USERPROFILE", "HOMEDRIVE",
    "HOMEPATH", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "PROGRAMFILES", "PROGRAMFILES(X86)",
    "COMMONPROGRAMFILES", "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "OS", "NVM_HOME",
    "NVM_SYMLINK", "CARGO_HOME", "RUSTUP_HOME", "GOPATH", "GOROOT", "GOCACHE", "JAVA_HOME",
];

#[derive(Debug, Clone, Default)]
pub struct SandboxPolicy {
    pub cpu_cores: Option<u32>,
    pub cpu_rate_percent: Option<u32>,
    pub memory_limit_mb: Option<u64>,
    pub low_priority: bool,
}

impl From<&AppSettings> for SandboxPolicy {
    fn from(s: &AppSettings) -> Self {
        let nz = |v: u32| (v > 0).then_some(v);
        Self {
            cpu_cores: nz(s.cpu_cores_limit),
            cpu_rate_percent: nz(s.cpu_rate_limit_percent),
            memory_limit_mb: nz(s.memory_hard_limit_mb).map(u64::from),
            low_priority: s.low_priority,
        }
    }
}

/// Handle ke proses yang berjalan di sandbox. Satu-satunya jalan untuk mematikan proses:
/// Frontend tidak pernah mengirim PID, sehingga tidak mungkin "salah bunuh" proses lain
/// akibat PID reuse — kita selalu memakai handle proses & Job Object milik anak kita sendiri.
pub struct Sandboxed {
    child: Child,
    guard: job::Guard,
    finished: bool,
}

impl Sandboxed {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Matikan seluruh pohon proses (anak + cucu) lewat Job Object.
    pub fn terminate(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        job::terminate(&mut self.child, &self.guard);
    }
}

impl Drop for Sandboxed {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub fn spawn(step: &Step, cwd: &Path, env: &[(String, String)], policy: &SandboxPolicy) -> Result<Sandboxed> {
    let program = resolve_program(&step.program, cwd)?;
    let mut cmd = Command::new(&program);
    if step.program == SELF_STATIC_SERVER {
        cmd.arg(STATIC_SERVER_FLAG);
    }
    cmd.args(&step.args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_env(&mut cmd);
    for (k, v) in env {
        cmd.env(k, v);
    }

    let (child, guard) = job::spawn(cmd, policy)
        .with_context(|| format!("Gagal menjalankan {}", program.display()))?;
    Ok(Sandboxed { child, guard, finished: false })
}

/// Menjalankan tool sistem tepercaya dengan path absolut tetap (mis. Windows Defender
/// `MpCmdRun.exe` untuk scan keamanan) — tetap di dalam Job Object, env bersih, tanpa shell,
/// prioritas rendah, tanpa output (tidak perlu dibaca → tidak ada risiko pipe penuh).
pub fn spawn_system(program: &Path, args: &[&str]) -> Result<Sandboxed> {
    if !program.is_absolute() {
        bail!("Tool sistem harus berupa path absolut");
    }
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    apply_env(&mut cmd);
    let policy = SandboxPolicy { low_priority: true, ..SandboxPolicy::default() };
    let (child, guard) = job::spawn(cmd, &policy)
        .with_context(|| format!("Gagal menjalankan {}", program.display()))?;
    Ok(Sandboxed { child, guard, finished: false })
}

/// `env_clear()` lalu hanya meneruskan variabel dari [ENV_ALLOWLIST].
fn apply_env(cmd: &mut Command) {
    cmd.env_clear();
    for (key, value) in std::env::vars_os() {
        let allowed = key
            .to_str()
            .is_some_and(|k| ENV_ALLOWLIST.iter().any(|a| a.eq_ignore_ascii_case(k)));
        if allowed {
            cmd.env(&key, &value);
        }
    }
}

/// Program harus: (a) tool dari allowlist yang ada di PATH, atau
/// (b) path absolut yang — setelah symlink di-resolve — berada di dalam direktori repo (venv).
fn resolve_program(program: &str, cwd: &Path) -> Result<PathBuf> {
    if program == SELF_STATIC_SERVER {
        return std::env::current_exe().context("Tidak dapat menemukan executable RepoLaunch");
    }
    let path = Path::new(program);
    if path.is_absolute() {
        return fsutil::ensure_within(cwd, path).context("Interpreter harus berada di dalam direktori repo");
    }
    if !ALLOWED_TOOLS.contains(&program) {
        bail!("Program '{program}' tidak ada di allowlist");
    }
    which::which(program).with_context(|| {
        format!("Runtime '{program}' tidak ditemukan di PATH. Instal terlebih dahulu lalu mulai ulang RepoLaunch.")
    })
}

// ───────────────────────────── Job Object ─────────────────────────────

mod job {
    use std::{
        ffi::c_void,
        io, mem,
        os::windows::{io::AsRawHandle, process::CommandExt},
        process::{Child, Command},
        ptr,
    };

    use anyhow::{bail, Result};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
            JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
            JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            JOB_OBJECT_LIMIT_AFFINITY, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
            JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PRIORITY_CLASS,
        },
    };

    use super::SandboxPolicy;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

    pub struct Guard {
        job: HANDLE,
    }
    // HANDLE adalah pointer mentah; Job Object handle aman dipakai lintas thread.
    unsafe impl Send for Guard {}
    unsafe impl Sync for Guard {}

    impl Drop for Guard {
        fn drop(&mut self) {
            // KILL_ON_JOB_CLOSE: menutup handle terakhir mematikan semua proses di Job.
            unsafe { CloseHandle(self.job) };
        }
    }

    fn create_job(policy: &SandboxPolicy) -> Result<Guard> {
        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                bail!("CreateJobObjectW gagal: {}", io::Error::last_os_error());
            }
            let guard = Guard { job };

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
            let basic = &mut info.BasicLimitInformation;
            basic.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
            if let Some(cores) = policy.cpu_cores {
                let total = std::thread::available_parallelism().map_or(1, |n| n.get()) as u32;
                let n = cores.clamp(1, total.min(usize::BITS));
                basic.Affinity = if n >= usize::BITS { usize::MAX } else { (1usize << n) - 1 };
                basic.LimitFlags |= JOB_OBJECT_LIMIT_AFFINITY;
            }
            if policy.low_priority {
                basic.PriorityClass = BELOW_NORMAL_PRIORITY_CLASS;
                basic.LimitFlags |= JOB_OBJECT_LIMIT_PRIORITY_CLASS;
            }
            if let Some(mb) = policy.memory_limit_mb {
                info.JobMemoryLimit = (mb * 1024 * 1024) as usize;
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            }
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                bail!("SetInformationJobObject (limit) gagal: {}", io::Error::last_os_error());
            }

            if let Some(rate) = policy.cpu_rate_percent {
                let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = mem::zeroed();
                cpu.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                cpu.Anonymous.CpuRate = rate.clamp(1, 100) * 100; // satuan 1/100 persen
                let ok = SetInformationJobObject(
                    job,
                    JobObjectCpuRateControlInformation,
                    &cpu as *const _ as *const c_void,
                    mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                );
                if ok == 0 {
                    bail!("SetInformationJobObject (cpu rate) gagal: {}", io::Error::last_os_error());
                }
            }
            Ok(guard)
        }
    }

    pub fn spawn(mut cmd: Command, policy: &SandboxPolicy) -> Result<(Child, Guard)> {
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
        let guard = create_job(policy)?;
        let mut child = cmd.spawn()?;
        // Anak-cucu yang dibuat SETELAH assign otomatis ikut Job. (Jendela race di antara
        // spawn dan assign sangat kecil; untuk menutupnya sepenuhnya gunakan CREATE_SUSPENDED
        // + ResumeThread via CreateProcessW langsung.)
        let ok = unsafe { AssignProcessToJobObject(guard.job, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            let err = io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            bail!("Gagal memasukkan proses ke Job Object: {err}");
        }
        Ok((child, guard))
    }

    pub fn terminate(child: &mut Child, guard: &Guard) {
        // Proses konsol tanpa konsol bersama tidak bisa menerima Ctrl+C secara andal,
        // jadi Windows langsung terminate seluruh Job (anak + cucu).
        unsafe { TerminateJobObject(guard.job, 1) };
        let _ = child.wait();
    }
}
