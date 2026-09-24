//! Deteksi proyek WEB, port bawaan, dan rencana eksekusi (LaunchPlan).
//!
//! RepoLaunch khusus repositori berbasis web. Yang didukung:
//!
//! | Kelompok   | Deteksi                                                      | Dijalankan dengan                         |
//! |------------|--------------------------------------------------------------|-------------------------------------------|
//! | Node.js    | package.json + framework web di dependencies/script          | `npm/pnpm/yarn/bun run <dev|start|…>`     |
//! | HTML statis| index.html (root, public/, dist/, docs/, build/, site/)      | server statis bawaan RepoLaunch           |
//! | PHP        | index.php / public/index.php; Laravel via `artisan`          | `php -S` / `php artisan serve`            |
//! | Python     | Django (manage.py), FastAPI, Flask                           | venv + runserver / uvicorn / flask run    |
//!
//! Repo di luar daftar ini ditolak saat instalasi ([`NOT_WEB_PROJECT`]).
//!
//! Keamanan: yang dieksekusi selalu program dari allowlist (`sandbox::ALLOWED_TOOLS`),
//! interpreter venv di dalam repo, atau server statis bawaan — dengan argumen `Vec<String>`
//! tanpa shell. package.json hanya dipakai untuk MEMILIH nama script dari daftar tetap.

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use super::{fsutil::read_small, sandbox::SELF_STATIC_SERVER};

pub const NOT_WEB_PROJECT: &str = "Repo ini bukan proyek web. RepoLaunch hanya mendukung proyek web: \
Node.js (Vite, Next, Nuxt, Astro, SvelteKit, Angular, React, Express, …), HTML statis, PHP/Laravel, \
atau Django/FastAPI/Flask.";

const MAX_MANIFEST: u64 = 1024 * 1024;
const STATIC_ROOTS: [&str; 6] = ["", "public", "dist", "docs", "build", "site"];
const PY_ENTRIES: [&str; 4] = ["main.py", "app.py", "server.py", "run.py"];

static RE_ENV_PORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*(?:export\s+)?(?:PORT|APP_PORT)\s*=\s*["']?(\d{2,5})"#).unwrap());
static RE_CLI_PORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:--port|-p)(?:=|\s+)(\d{2,5})").unwrap());
static RE_CFG_PORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bport\s*[:=]\s*(\d{2,5})").unwrap());
static RE_FASTAPI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(\w+)\s*=\s*FastAPI\(").unwrap());
static RE_FLASK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(\w+)\s*=\s*Flask\(").unwrap());

// ───────────────────────────── Tipe ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeFw {
    Vite,
    SvelteKit,
    Remix,
    Astro,
    Next,
    Nuxt,
    Angular,
    Gatsby,
    Cra,
    Parcel,
    Webpack,
    /// Express/Fastify/Koa/Hono/NestJS/serve/http-server — port lewat env `PORT`.
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebStack {
    Node(NodeFw),
    /// Direktori root dokumen relatif terhadap repo ("" = root repo).
    Static(String),
    Php(String),
    Laravel,
    Django,
    /// target uvicorn, mis. "main:app"
    FastApi(String),
    /// target `flask --app`, mis. "app:app"
    Flask(String),
}

impl WebStack {
    pub fn label(&self) -> &'static str {
        match self {
            WebStack::Node(fw) => match fw {
                NodeFw::Vite => "Vite",
                NodeFw::SvelteKit => "SvelteKit",
                NodeFw::Remix => "Remix",
                NodeFw::Astro => "Astro",
                NodeFw::Next => "Next.js",
                NodeFw::Nuxt => "Nuxt",
                NodeFw::Angular => "Angular",
                NodeFw::Gatsby => "Gatsby",
                NodeFw::Cra => "Create React App",
                NodeFw::Parcel => "Parcel",
                NodeFw::Webpack => "Webpack",
                NodeFw::Server => "Node.js server",
            },
            WebStack::Static(_) => "HTML statis",
            WebStack::Php(_) => "PHP",
            WebStack::Laravel => "Laravel",
            WebStack::Django => "Django",
            WebStack::FastApi(_) => "FastAPI",
            WebStack::Flask(_) => "Flask",
        }
    }

    fn default_port(&self) -> u16 {
        match self {
            WebStack::Node(fw) => match fw {
                NodeFw::Vite | NodeFw::SvelteKit | NodeFw::Remix => 5173,
                NodeFw::Astro => 4321,
                NodeFw::Angular => 4200,
                NodeFw::Gatsby => 8000,
                NodeFw::Parcel => 1234,
                NodeFw::Webpack => 8080,
                NodeFw::Next | NodeFw::Nuxt | NodeFw::Cra | NodeFw::Server => 3000,
            },
            WebStack::Static(_) => 8080,
            WebStack::Flask(_) => 5000,
            WebStack::Php(_) | WebStack::Laravel | WebStack::Django | WebStack::FastApi(_) => 8000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    /// Tool dari allowlist (`npm`), path absolut di dalam repo (`.venv\Scripts\python.exe`),
    /// atau [`SELF_STATIC_SERVER`].
    pub program: String,
    pub args: Vec<String>,
    /// `false` = langkah setup yang ditunggu sampai selesai; `true` = server utama.
    pub long_running: bool,
}

impl Step {
    fn new<I, S>(label: impl Into<String>, program: impl Into<String>, args: I, long_running: bool) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            label: label.into(),
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            long_running,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub stack: WebStack,
    pub steps: Vec<Step>,
    pub env: Vec<(String, String)>,
}

// ───────────────────────────── Deteksi ─────────────────────────────

pub fn detect_stack(dir: &Path) -> Option<WebStack> {
    let has = |f: &str| dir.join(f).is_file();

    if let Some(pkg) = read_package_json(dir) {
        if let Some(fw) = node_framework(&pkg) {
            if pick_script(&pkg).is_some() {
                return Some(WebStack::Node(fw));
            }
        }
    }
    if has("artisan") && has("composer.json") {
        return Some(WebStack::Laravel);
    }
    for root in ["", "public"] {
        if dir.join(root).join("index.php").is_file() {
            return Some(WebStack::Php(root.to_string()));
        }
    }
    if has("manage.py") {
        return Some(WebStack::Django);
    }
    if let Some(py) = python_web(dir) {
        return Some(py);
    }
    // Paling akhir: situs statis (termasuk repo Node tanpa framework yang punya index.html).
    STATIC_ROOTS
        .iter()
        .find(|r| dir.join(r).join("index.html").is_file())
        .map(|r| WebStack::Static(r.to_string()))
}

fn read_package_json(dir: &Path) -> Option<Value> {
    read_small(&dir.join("package.json"), MAX_MANIFEST).and_then(|s| serde_json::from_str(&s).ok())
}

/// Hanya nama script dari daftar tetap ini yang bisa dijalankan.
fn pick_script(pkg: &Value) -> Option<(&'static str, String)> {
    let scripts = pkg.get("scripts")?.as_object()?;
    ["dev", "start", "serve", "preview"]
        .into_iter()
        .find_map(|name| scripts.get(name).and_then(Value::as_str).map(|cmd| (name, cmd.to_string())))
}

fn node_framework(pkg: &Value) -> Option<NodeFw> {
    let has_dep = |name: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .any(|s| pkg.get(s).and_then(Value::as_object).is_some_and(|d| d.contains_key(name)))
    };
    let script = pick_script(pkg).map(|(_, c)| c).unwrap_or_default();
    let uses = |cli: &str| script_uses(&script, cli);

    let fw = if has_dep("next") || uses("next") {
        NodeFw::Next
    } else if has_dep("nuxt") || uses("nuxt") || uses("nuxi") {
        NodeFw::Nuxt
    } else if has_dep("astro") || uses("astro") {
        NodeFw::Astro
    } else if has_dep("@sveltejs/kit") {
        NodeFw::SvelteKit
    } else if has_dep("@remix-run/dev") || has_dep("@remix-run/react") {
        NodeFw::Remix
    } else if has_dep("@angular/core") || uses("ng") {
        NodeFw::Angular
    } else if has_dep("gatsby") || uses("gatsby") {
        NodeFw::Gatsby
    } else if has_dep("react-scripts") || uses("react-scripts") {
        NodeFw::Cra
    } else if has_dep("vite") || uses("vite") {
        NodeFw::Vite
    } else if has_dep("parcel") || uses("parcel") {
        NodeFw::Parcel
    } else if has_dep("webpack-dev-server") || uses("webpack") {
        NodeFw::Webpack
    } else if ["express", "fastify", "koa", "hono", "@nestjs/core", "@hapi/hapi", "serve", "http-server", "live-server", "browser-sync"]
        .iter()
        .any(|d| has_dep(d))
    {
        NodeFw::Server
    } else {
        return None;
    };
    Some(fw)
}

/// Apakah script memanggil CLI `cli` sebagai perintah (bukan sekadar substring).
fn script_uses(script: &str, cli: &str) -> bool {
    script
        .split(|c: char| c.is_whitespace() || matches!(c, '&' | ';' | '|' | '(' | ')'))
        .any(|t| t == cli)
}

fn python_web(dir: &Path) -> Option<WebStack> {
    for entry in PY_ENTRIES {
        let Some(src) = read_small(&dir.join(entry), MAX_MANIFEST) else { continue };
        let module = entry.trim_end_matches(".py");
        if let Some(c) = RE_FASTAPI.captures(&src) {
            return Some(WebStack::FastApi(format!("{module}:{}", &c[1])));
        }
        if let Some(c) = RE_FLASK.captures(&src) {
            return Some(WebStack::Flask(format!("{module}:{}", &c[1])));
        }
    }
    None
}

// ───────────────────────────── Port bawaan ─────────────────────────────

fn parse_port(s: &str) -> Option<u16> {
    s.parse::<u16>().ok().filter(|p| *p > 0)
}

/// Urutan prioritas: file .env → flag di script package.json → vite.config → default framework.
pub fn default_port(dir: &Path) -> Option<u16> {
    let stack = detect_stack(dir)?;
    for f in [".env.local", ".env.development", ".env"] {
        if let Some(text) = read_small(&dir.join(f), 256 * 1024) {
            if let Some(p) = RE_ENV_PORT.captures(&text).and_then(|c| parse_port(&c[1])) {
                return Some(p);
            }
        }
    }
    if let WebStack::Node(_) = stack {
        if let Some((_, cmd)) = read_package_json(dir).as_ref().and_then(pick_script) {
            if let Some(p) = RE_CLI_PORT.captures(&cmd).and_then(|c| parse_port(&c[1])) {
                return Some(p);
            }
        }
        for cfg in ["vite.config.ts", "vite.config.js", "vite.config.mjs", "vite.config.mts"] {
            if let Some(text) = read_small(&dir.join(cfg), MAX_MANIFEST) {
                if let Some(p) = RE_CFG_PORT.captures(&text).and_then(|c| parse_port(&c[1])) {
                    return Some(p);
                }
            }
        }
    }
    Some(stack.default_port())
}

// ───────────────────────────── Rencana eksekusi ─────────────────────────────

pub fn build_plan(dir: &Path, port: u16, allow_install_scripts: bool) -> Result<LaunchPlan> {
    let stack = detect_stack(dir).ok_or_else(|| anyhow!(NOT_WEB_PROJECT))?;
    let port_s = port.to_string();
    let mut env = vec![
        ("PORT".to_string(), port_s.clone()),
        ("HOST".to_string(), "127.0.0.1".to_string()),
    ];
    let steps = match &stack {
        WebStack::Node(fw) => node_steps(dir, *fw, &port_s, allow_install_scripts, &mut env)?,
        WebStack::Static(root) => vec![Step::new(
            "Menjalankan server statis",
            SELF_STATIC_SERVER,
            [dir.join(root).to_string_lossy().into_owned(), port_s.clone()],
            true,
        )],
        WebStack::Php(root) => {
            let docroot = if root.is_empty() { ".".to_string() } else { root.clone() };
            let mut steps = composer_steps(dir, allow_install_scripts);
            let addr = format!("127.0.0.1:{port}");
            steps.push(Step::new("Menjalankan php -S", "php", ["-S", addr.as_str(), "-t", docroot.as_str()], true));
            steps
        }
        WebStack::Laravel => {
            let mut steps = composer_steps(dir, allow_install_scripts);
            steps.push(Step::new(
                "Menjalankan php artisan serve",
                "php",
                vec!["artisan".to_string(), "serve".into(), "--host=127.0.0.1".into(), format!("--port={port}")],
                true,
            ));
            steps
        }
        WebStack::Django | WebStack::FastApi(_) | WebStack::Flask(_) => python_steps(dir, &stack, &port_s, &mut env),
    };
    Ok(LaunchPlan { stack, steps, env })
}

// Node.js ------------------------------------------------------------------

impl NodeFw {
    /// (nama CLI yang harus dipanggil langsung oleh script, flag port).
    fn port_flags(self, port: &str) -> Option<(Vec<&'static str>, Vec<String>)> {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let strict = v(&["--port", port, "--strictPort", "--host", "127.0.0.1"]);
        Some(match self {
            NodeFw::Vite | NodeFw::SvelteKit => (vec!["vite"], strict),
            NodeFw::Remix => (vec!["vite", "remix"], strict),
            NodeFw::Astro => (vec!["astro"], strict),
            NodeFw::Next => (vec!["next"], v(&["-p", port, "-H", "127.0.0.1"])),
            NodeFw::Nuxt => (vec!["nuxt", "nuxi"], v(&["--port", port, "--host", "127.0.0.1"])),
            NodeFw::Angular => (vec!["ng"], v(&["--port", port, "--host", "127.0.0.1"])),
            NodeFw::Gatsby => (vec!["gatsby"], v(&["-p", port, "-H", "127.0.0.1"])),
            NodeFw::Parcel => (vec!["parcel"], v(&["--port", port])),
            NodeFw::Webpack => (vec!["webpack", "webpack-dev-server"], v(&["--port", port, "--host", "127.0.0.1"])),
            NodeFw::Cra | NodeFw::Server => return None, // cukup env PORT/HOST
        })
    }
}

fn package_manager(dir: &Path) -> &'static str {
    if dir.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if dir.join("yarn.lock").is_file() {
        "yarn"
    } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() {
        "bun"
    } else {
        "npm"
    }
}

fn node_steps(dir: &Path, fw: NodeFw, port: &str, allow_scripts: bool, env: &mut Vec<(String, String)>) -> Result<Vec<Step>> {
    let pkg = read_package_json(dir).ok_or_else(|| anyhow!("package.json tidak valid"))?;
    let (script, cmd) =
        pick_script(&pkg).ok_or_else(|| anyhow!("package.json tidak punya script dev/start/serve/preview"))?;
    let pm = package_manager(dir);
    let mut steps = Vec::new();

    if !dir.join("node_modules").is_dir() {
        let mut args = vec!["install".to_string()];
        if !allow_scripts {
            args.push("--ignore-scripts".into());
        }
        steps.push(Step::new("Menginstal dependensi", pm, args, false));
    }

    let mut args = vec!["run".to_string(), script.to_string()];
    // Flag port hanya ditambahkan jika script memanggil CLI framework secara langsung
    // (mis. "vite", bukan "npm-run-all …") dan belum menetapkan port yang sama.
    let already = RE_CLI_PORT.captures(&cmd).is_some_and(|c| &c[1] == port);
    if let Some((clis, flags)) = fw.port_flags(port) {
        if !already && clis.iter().any(|cli| script_uses(&cmd, cli)) {
            if pm == "npm" {
                args.push("--".into());
            }
            args.extend(flags);
        }
    }
    env.push(("BROWSER".into(), "none".into())); // CRA: jangan buka browser otomatis
    env.push(("NG_CLI_ANALYTICS".into(), "false".into()));
    steps.push(Step::new(format!("Menjalankan {pm} run {script}"), pm, args, true));
    Ok(steps)
}

// PHP ----------------------------------------------------------------------

fn composer_steps(dir: &Path, allow_scripts: bool) -> Vec<Step> {
    if !dir.join("composer.json").is_file() || dir.join("vendor").is_dir() {
        return Vec::new();
    }
    let mut args = vec!["install", "--no-interaction", "--no-progress"];
    if !allow_scripts {
        args.push("--no-scripts");
    }
    vec![Step::new("Menginstal dependensi Composer", "composer", args, false)]
}

// Python -------------------------------------------------------------------

fn venv_python(dir: &Path) -> PathBuf {
    dir.join(".venv").join("Scripts").join("python.exe")
}

fn python_steps(dir: &Path, stack: &WebStack, port: &str, env: &mut Vec<(String, String)>) -> Vec<Step> {
    let venv_py = venv_python(dir);
    let py = venv_py.to_string_lossy().into_owned();
    let mut steps = Vec::new();

    // Dependensi diisolasi di .venv milik repo, tidak menyentuh Python sistem.
    if !venv_py.is_file() {
        steps.push(Step::new("Membuat virtualenv", "python", ["-m", "venv", ".venv"], false));
    }
    if dir.join("requirements.txt").is_file() {
        steps.push(Step::new(
            "Menginstal requirements.txt",
            py.clone(),
            ["-m", "pip", "install", "-r", "requirements.txt"],
            false,
        ));
    }
    env.push(("PYTHONUNBUFFERED".into(), "1".into()));
    env.push(("PIP_DISABLE_PIP_VERSION_CHECK".into(), "1".into()));

    let run: Vec<String> = match stack {
        WebStack::FastApi(t) => ["-m", "uvicorn", t.as_str(), "--host", "127.0.0.1", "--port", port].map(String::from).to_vec(),
        WebStack::Flask(t) => {
            ["-m", "flask", "--app", t.as_str(), "run", "--host", "127.0.0.1", "--port", port].map(String::from).to_vec()
        }
        _ => vec!["manage.py".into(), "runserver".into(), format!("127.0.0.1:{port}")],
    };
    steps.push(Step::new(format!("Menjalankan {}", stack.label()), py, run, true));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (p, body) in files {
            let path = d.path().join(p);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        d
    }

    #[test]
    fn vite_plan_injects_port() {
        let d = dir(&[("package.json", r#"{"scripts":{"dev":"vite"},"devDependencies":{"vite":"^6"}}"#)]);
        fs::create_dir(d.path().join("node_modules")).unwrap();
        assert_eq!(detect_stack(d.path()), Some(WebStack::Node(NodeFw::Vite)));
        assert_eq!(default_port(d.path()), Some(5173));

        let plan = build_plan(d.path(), 5174, false).unwrap();
        assert_eq!(plan.steps.len(), 1, "node_modules ada → tanpa install");
        let serve = &plan.steps[0];
        assert_eq!(&serve.args[..3], ["run", "dev", "--"]);
        assert!(serve.args.contains(&"5174".to_string()));
    }

    #[test]
    fn next_install_ignores_scripts() {
        let d = dir(&[("package.json", r#"{"scripts":{"dev":"next dev"},"dependencies":{"next":"15"}}"#)]);
        let plan = build_plan(d.path(), 3001, false).unwrap();
        assert!(plan.steps[0].args.contains(&"--ignore-scripts".to_string()));
        assert!(plan.steps[1].args.contains(&"-p".to_string()));
    }

    #[test]
    fn wrapped_script_gets_env_only() {
        let d = dir(&[("package.json", r#"{"scripts":{"dev":"npm-run-all -p api web"},"devDependencies":{"vite":"^6"}}"#)]);
        let plan = build_plan(d.path(), 5200, false).unwrap();
        assert_eq!(plan.steps.last().unwrap().args, ["run", "dev"]);
        assert!(plan.env.contains(&("PORT".into(), "5200".into())));
    }

    #[test]
    fn static_site_uses_builtin_server() {
        let d = dir(&[("public/index.html", "<h1>hi</h1>")]);
        assert_eq!(detect_stack(d.path()), Some(WebStack::Static("public".into())));
        let plan = build_plan(d.path(), 8081, false).unwrap();
        assert_eq!(plan.steps[0].program, SELF_STATIC_SERVER);
    }

    #[test]
    fn php_and_laravel() {
        let php = dir(&[("index.php", "<?php echo 1;")]);
        assert_eq!(detect_stack(php.path()), Some(WebStack::Php(String::new())));
        let laravel = dir(&[("artisan", ""), ("composer.json", "{}"), ("public/index.php", "")]);
        assert_eq!(detect_stack(laravel.path()), Some(WebStack::Laravel));
    }

    #[test]
    fn detects_fastapi() {
        let d = dir(&[("main.py", "from fastapi import FastAPI\napi = FastAPI()\n")]);
        assert_eq!(detect_stack(d.path()), Some(WebStack::FastApi("main:api".into())));
    }

    #[test]
    fn rejects_non_web_projects() {
        for files in [
            &[("Cargo.toml", "[package]")][..],
            &[("go.mod", "module x")][..],
            &[("main.py", "print('cli tool')")][..],
            &[("package.json", r#"{"scripts":{"start":"node cli.js"},"dependencies":{"chalk":"5"}}"#)][..],
        ] {
            let d = dir(files);
            assert_eq!(detect_stack(d.path()), None, "{files:?}");
        }
    }
}
