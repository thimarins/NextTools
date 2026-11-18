//! `nexttool` — command line for NextSize and NextCopy. Same engines and the
//! same task manager the desktop app will use.
//!
//! Exit codes: 0 ok · 1 completed with warnings · 2 failed · 3 interrupted /
//! paused / cancelled · 64 usage or validation error.

mod gen;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand, ValueEnum};
use nexttool_core::fs::{make_enumerator, Backend};
use nexttool_core::task::{Module, TaskState};
use nexttool_core::units::{format_bytes, format_count};
use nexttool_store::{default_data_dir, init_logging, AppStore, TaskRecord};
use nexttool_tasks::runners::default_factory;
use nexttool_tasks::{CopySpec, SizeScanSpec, TaskManager, TaskSpec};
use nextcopy_core::{CopyJob, CopyProfile, VerifyLevel};
use nextsize_core::{NullSink, Profile, ScanOptions, ScanProgress, ScanSink, Tuning};
use nextsize_index::{FileQuery, IndexWriter, Snapshot};
use nextsize_scanner::{run_scan, ScanControl, ScanStart};

const EXIT_OK: i32 = 0;
const EXIT_WARN: i32 = 1;
const EXIT_FAIL: i32 = 2;
const EXIT_INTERRUPTED: i32 = 3;
const EXIT_USAGE: i32 = 64;

#[derive(Parser)]
#[command(name = "nexttool-cli", version, about = "NextTool — NextSize (análise) e NextCopy (cópia)")]
struct Cli {
    /// Diretório de dados (padrão: %LOCALAPPDATA%\NextTool, ou .\data no modo portátil)
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Saída JSON (para scripts)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// NextSize: análise de armazenamento
    #[command(subcommand)]
    Size(SizeCmd),
    /// NextCopy: cópia, sincronização e verificação
    #[command(subcommand)]
    Copy(CopyCmd),
    /// Tarefas e histórico
    #[command(subcommand)]
    Tasks(TasksCmd),
    /// Central de alertas
    #[command(subcommand)]
    Alerts(AlertsCmd),
    /// Configurações do aplicativo (as mesmas da tela Configurações)
    #[command(subcommand)]
    Settings(SettingsCmd),
    /// Recursos do processo (diagnóstico)
    Sys,
}

#[derive(Subcommand)]
enum SettingsCmd {
    List,
    Set { key: String, value: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum ProfileArg {
    Economy,
    Balanced,
    Fast,
}

impl From<ProfileArg> for Profile {
    fn from(p: ProfileArg) -> Self {
        match p {
            ProfileArg::Economy => Profile::Economy,
            ProfileArg::Balanced => Profile::Balanced,
            ProfileArg::Fast => Profile::Fast,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum BackendArg {
    Auto,
    Native,
    Portable,
}

impl From<BackendArg> for Backend {
    fn from(b: BackendArg) -> Self {
        match b {
            BackendArg::Auto => Backend::Auto,
            BackendArg::Native => Backend::Native,
            BackendArg::Portable => Backend::Portable,
        }
    }
}

#[derive(Subcommand)]
enum SizeCmd {
    /// Analisa um diretório/volume/compartilhamento e grava um snapshot (.nxs).
    /// Se o snapshot existir e estiver interrompido, a análise é retomada.
    Scan {
        root: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, value_enum, default_value = "balanced")]
        profile: ProfileArg,
        #[arg(long, value_enum, default_value = "auto")]
        backend: BackendArg,
        #[arg(long)]
        threads: Option<usize>,
    },
    /// Resumo do snapshot (instantâneo: lê apenas metadados)
    Info { snapshot: PathBuf },
    /// Maiores arquivos
    Top { snapshot: PathBuf, #[arg(long, default_value_t = 20)] n: u32 },
    /// Maiores diretórios (tamanho total)
    Dirs { snapshot: PathBuf, #[arg(long, default_value_t = 20)] n: u32 },
    /// Conteúdo de um diretório (paginado)
    Ls { snapshot: PathBuf, #[arg(long, default_value_t = 1)] dir: i64, #[arg(long, default_value_t = 0)] offset: u32, #[arg(long, default_value_t = 50)] limit: u32 },
    /// Consulta filtrada (paginada, com tempo limite)
    Query(QueryArgs),
    /// Distribuição por extensão e por ano
    Stats { snapshot: PathBuf, #[arg(long, default_value_t = 20)] n: u32 },
    /// Erros registrados na análise
    Issues { snapshot: PathBuf, #[arg(long, default_value_t = 50)] limit: u32 },
    /// Verifica a integridade do snapshot
    Check { snapshot: PathBuf },
    /// Gera massa de teste sintética
    Gen {
        dir: PathBuf,
        #[arg(long)]
        files: u64,
        #[arg(long, default_value_t = 200)]
        per_dir: u64,
        #[arg(long, default_value_t = 8)]
        fanout: u64,
        #[arg(long, default_value_t = 512)]
        max_size: u64,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Benchmark do motor (tempo, taxa, RAM, CPU, tamanho do índice, consultas)
    Bench(BenchArgs),
}

#[derive(Args)]
struct QueryArgs {
    snapshot: PathBuf,
    #[arg(long)]
    min_size: Option<u64>,
    #[arg(long)]
    max_size: Option<u64>,
    #[arg(long)]
    ext: Option<String>,
    /// Modificados antes deste ano (ex.: 2020)
    #[arg(long)]
    before_year: Option<i32>,
    #[arg(long)]
    year: Option<i32>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long, default_value_t = 0)]
    offset: u32,
    #[arg(long, default_value_t = 50)]
    limit: u32,
    #[arg(long, default_value_t = 10)]
    timeout_s: u64,
}

#[derive(Args)]
struct BenchArgs {
    root: PathBuf,
    #[arg(long, value_enum, default_value = "native")]
    backend: BackendArg,
    #[arg(long, value_enum, default_value = "balanced")]
    profile: ProfileArg,
    #[arg(long)]
    threads: Option<usize>,
    /// Sem persistência (mede só a enumeração)
    #[arg(long)]
    no_persist: bool,
    /// Onde gravar o índice temporário
    #[arg(long)]
    db_dir: Option<PathBuf>,
    #[arg(long)]
    keep_db: bool,
    #[arg(long, default_value_t = 1)]
    repeat: u32,
    /// Acrescenta o resultado (JSON por linha) neste arquivo
    #[arg(long)]
    json_out: Option<PathBuf>,
    /// Rótulo livre do cenário (ex.: "synthetic-1M", "C:\\ real")
    #[arg(long, default_value = "")]
    label: String,
}

#[derive(Args)]
struct CopyArgs {
    #[arg(long)]
    src: PathBuf,
    #[arg(long)]
    dst: PathBuf,
    #[arg(long, default_value = "safe")]
    profile: String,
    #[arg(long)]
    threads: Option<u32>,
    #[arg(long, value_parser = ["basic", "standard", "strict"])]
    verify: Option<String>,
    /// Arquivos individuais (relativos à origem), separados por vírgula
    #[arg(long, value_delimiter = ',')]
    files: Vec<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    exclude_files: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    exclude_dirs: Vec<String>,
    /// Número de exclusões aprovadas após `copy plan` (obrigatório para espelhamento)
    #[arg(long)]
    approve_deletions: Option<u64>,
}

impl CopyArgs {
    fn job(&self) -> Result<CopyJob, String> {
        let profile = CopyProfile::parse(&self.profile).ok_or_else(|| format!("perfil desconhecido: {}", self.profile))?;
        let mut job = CopyJob::new(self.src.clone(), self.dst.clone(), profile);
        if let Some(t) = self.threads {
            job.options.threads = t;
        }
        if let Some(v) = &self.verify {
            job.options.verify = match v.as_str() {
                "basic" => VerifyLevel::Basic,
                "strict" => VerifyLevel::Strict,
                _ => VerifyLevel::Standard,
            };
        }
        job.files = self.files.clone();
        job.options.filters.exclude_files = self.exclude_files.clone();
        job.options.filters.exclude_dirs = self.exclude_dirs.clone();
        job.approved_deletions = self.approve_deletions;
        job.validate().map_err(|e| e.to_string())?;
        Ok(job)
    }
}

#[derive(Subcommand)]
enum CopyCmd {
    /// Prévia (dry-run, /L): o que seria copiado e EXCLUÍDO. Não altera nada.
    Plan(CopyArgs),
    /// Executa a cópia como tarefa (fila, retentativas, verificação, histórico)
    Run(CopyArgs),
    /// Verifica destino × origem sem copiar
    Verify {
        #[arg(long)]
        src: PathBuf,
        #[arg(long)]
        dst: PathBuf,
        #[arg(long)]
        strict: bool,
    },
}

#[derive(Subcommand)]
enum TasksCmd {
    List { #[arg(long, default_value_t = 30)] limit: u32 },
    Show { id: i64 },
    /// Recoloca na fila e executa (retomada)
    Resume { id: i64 },
    Cancel { id: i64 },
}

#[derive(Subcommand)]
enum AlertsCmd {
    List {
        #[arg(long)]
        all: bool,
    },
    Ack { id: i64 },
}

fn main() {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.clone().unwrap_or_else(default_data_dir);
    if let Err(e) = init_logging(&data_dir.join("logs"), "warn") {
        eprintln!("aviso: não foi possível iniciar o log em arquivo: {e}");
    }
    let code = match run(cli, &data_dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("erro: {e}");
            EXIT_USAGE
        }
    };
    std::process::exit(code);
}

fn open_store(data_dir: &Path) -> Result<AppStore, String> {
    let (store, note) = AppStore::open(&data_dir.join("app.db")).map_err(|e| e.to_string())?;
    if let Some(n) = note {
        eprintln!("ATENÇÃO: {n}");
        let d = nexttool_tasks::alerts::for_error(Module::System, 0, nexttool_core::ErrorKind::CorruptState, "app.db", false, n);
        let _ = store.raise_alert(&d);
    }
    // Startup recovery (crash / power loss / forced termination).
    match store.recover_interrupted() {
        Ok(list) => {
            for r in list.iter().filter(|r| !r.owned_by_live_process) {
                eprintln!(
                    "tarefa {} \"{}\" foi interrompida inesperadamente{} — use `nexttool-cli tasks resume {}` para retomar",
                    r.task.id,
                    r.task.title,
                    if r.child_still_running { " (processo filho ainda ativo!)" } else { "" },
                    r.task.id
                );
                let _ = store.raise_alert(&nexttool_tasks::alerts::interrupted(r.task.module, r.task.id, &r.task.title, r.child_still_running));
            }
        }
        Err(e) => eprintln!("aviso: falha na recuperação de tarefas: {e}"),
    }
    Ok(store)
}

fn manager(data_dir: &Path) -> Result<Arc<TaskManager>, String> {
    let store = open_store(data_dir)?;
    Ok(Arc::new(TaskManager::new(store, data_dir.to_path_buf(), default_factory(), 2)))
}

fn install_ctrlc(mgr: &Arc<TaskManager>) {
    let m = mgr.clone();
    let hits = Arc::new(AtomicU32::new(0));
    let _ = ctrlc::set_handler(move || {
        let n = hits.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            eprintln!("\ninterrupção solicitada: parando com segurança (a tarefa poderá ser retomada). Ctrl+C novamente força a saída.");
            for id in m.running_ids() {
                m.pause(id);
            }
        } else {
            eprintln!("saída forçada; a tarefa será marcada como interrompida na próxima execução.");
            std::process::exit(EXIT_INTERRUPTED);
        }
    });
}

fn exit_for(state: TaskState) -> i32 {
    match state {
        TaskState::Completed => EXIT_OK,
        TaskState::CompletedWithWarnings => EXIT_WARN,
        TaskState::Failed => EXIT_FAIL,
        _ => EXIT_INTERRUPTED,
    }
}

fn run_task(data_dir: &Path, spec: TaskSpec, title: String, json: bool) -> Result<i32, String> {
    let mgr = manager(data_dir)?;
    install_ctrlc(&mgr);
    let id = mgr.submit(spec.to_new_task(title)).map_err(|e| e.to_string())?;
    if !json {
        eprintln!("tarefa {id} criada");
    }
    let mut last = Instant::now() - Duration::from_secs(10);
    let recs = loop {
        let recs = mgr
            .run_until_done(&[id], &mut |r| {
                if !json && last.elapsed() >= Duration::from_millis(500) {
                    last = Instant::now();
                    if let Some(p) = &r[0].progress_json {
                        eprint!("\r{} {}", r[0].state.as_str(), progress_line(p));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        // A waiting retry keeps the CLI alive until it runs or gives up.
        if recs[0].state == TaskState::WaitingRetry {
            if !json {
                eprintln!("\naguardando nova tentativa ({})…", recs[0].error_message.clone().unwrap_or_default());
            }
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }
        break recs;
    };
    let r = &recs[0];
    if json {
        println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
    } else {
        eprintln!();
        print_task(r);
    }
    Ok(exit_for(r.state))
}

fn progress_line(p: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(p).unwrap_or_default();
    if v.get("files").is_some() {
        format!(
            "arquivos {} | dirs {} | {} | pendentes {} | erros {} | RAM {}   ",
            format_count(v["files"].as_u64().unwrap_or(0)),
            format_count(v["dirs"].as_u64().unwrap_or(0)),
            format_bytes(v["bytes"].as_u64().unwrap_or(0)),
            format_count(v["pending_dirs"].as_u64().unwrap_or(0)),
            v["issues"].as_u64().unwrap_or(0),
            format_bytes(nexttool_core::process::self_stats().working_set),
        )
    } else if v.get("phase").and_then(|x| x.as_str()) == Some("verify") {
        "verificando…   ".into()
    } else {
        format!(
            "arquivos {} | {} | {:.1} MB/s | erros {} | {}   ",
            format_count(v["files_done"].as_u64().unwrap_or(0)),
            format_bytes(v["io_write"].as_u64().unwrap_or(0)),
            v["rate_mb_s"].as_f64().unwrap_or(0.0),
            v["errors"].as_u64().unwrap_or(0),
            v["activity"].as_str().unwrap_or("")
        )
    }
}

fn print_task(r: &TaskRecord) {
    println!("tarefa {} [{}] {}", r.id, r.state.as_str(), r.title);
    println!("  execuções: {} (limite de retentativas automáticas: {})", r.attempt, r.max_attempts);
    if let Some(e) = &r.error_message {
        println!("  erro ({}): {e}", r.error_kind.clone().unwrap_or_default());
    }
    if let Some(res) = &r.result_json {
        let v: serde_json::Value = serde_json::from_str(res).unwrap_or_default();
        if let Some(st) = v.get("stats") {
            println!(
                "  arquivos: total {} · copiados {} · ignorados {} · falhas {} · extras {}",
                st["files_total"], st["files_copied"], st["files_skipped"], st["files_failed"], st["files_extra"]
            );
            println!("  bytes copiados: {}", format_bytes(st["bytes_copied"].as_u64().unwrap_or(0)));
        }
        if let Some(vr) = v.get("verify") {
            println!(
                "  verificação {}: {} conferidos · {} ok · ausentes {} · tamanho {} · data {} · hash {} · mantidos pela política {} · não verificados {}",
                vr["level"], vr["files_checked"], vr["files_ok"], vr["missing"], vr["size_mismatch"], vr["time_mismatch"], vr["hash_mismatch"], vr["kept_by_policy"], vr["not_verified"]
            );
        }
        if let Some(o) = v.get("outcome") {
            if o.get("files").is_some() {
                println!(
                    "  análise: {} arquivos · {} diretórios · {} · {} ocorrências · {} ms (+{} ms finalização)",
                    o["files"], o["dirs"], format_bytes(o["bytes"].as_u64().unwrap_or(0)), o["issues"], v["scan_ms"], v["finish_ms"]
                );
            }
        }
    }
}

fn run(cli: Cli, data_dir: &Path) -> Result<i32, String> {
    let json = cli.json;
    match cli.cmd {
        Cmd::Size(c) => size(c, data_dir, json),
        Cmd::Copy(c) => copy(c, data_dir, json),
        Cmd::Tasks(c) => tasks(c, data_dir, json),
        Cmd::Alerts(c) => alerts(c, data_dir, json),
        Cmd::Settings(c) => {
            let s = open_store(data_dir)?;
            match c {
                SettingsCmd::List => {
                    for (k, _, expected) in nexttool_store::SETTINGS {
                        let v = s.get_setting(k).map_err(|e| e.to_string())?;
                        println!("{k:<12} = {:<10} ({expected})", v.unwrap_or_else(|| "(padrão)".into()));
                    }
                }
                SettingsCmd::Set { key, value } => {
                    nexttool_store::validate_setting(&key, &value).map_err(|e| e.to_string())?;
                    s.set_setting(&key, &value).map_err(|e| e.to_string())?;
                    println!("{key} = {value}");
                }
            }
            Ok(EXIT_OK)
        }
        Cmd::Sys => {
            println!("{}", serde_json::to_string_pretty(&nexttool_core::process::self_stats()).unwrap_or_default());
            Ok(EXIT_OK)
        }
    }
}

fn abs(p: &Path) -> Result<PathBuf, String> {
    let a = std::path::absolute(p).map_err(|e| e.to_string())?;
    nexttool_core::security::validate_user_path(&a).map_err(|e| e.to_string())
}

fn out<T: serde::Serialize>(json: bool, v: &T, human: impl FnOnce(&T)) {
    if json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    } else {
        human(v);
    }
}

fn size(c: SizeCmd, data_dir: &Path, json: bool) -> Result<i32, String> {
    let snap = |p: &Path| Snapshot::open(p).map_err(|e| e.to_string());
    match c {
        SizeCmd::Scan { root, out, profile, backend, threads } => {
            let root = abs(&root)?;
            let out = std::path::absolute(&out).map_err(|e| e.to_string())?;
            let spec = TaskSpec::SizeScan(SizeScanSpec { root: root.clone(), snapshot: out, profile: profile.into(), backend: backend.into(), threads });
            run_task(data_dir, spec, format!("Análise {}", root.display()), json)
        }
        SizeCmd::Info { snapshot } => {
            let s = snap(&snapshot)?.summary().map_err(|e| e.to_string())?;
            out(json, &s, |s| {
                println!("origem:      {} ({})", s.root, s.source_kind);
                println!("estado:      {}{}", s.state, if s.complete { "" } else { "  ⚠ DADOS PARCIAIS" });
                println!("arquivos:    {}", format_count(s.files));
                println!("diretórios:  {}", format_count(s.dirs));
                println!("tam. lógico: {} ({} bytes)", format_bytes(s.bytes), s.bytes);
                println!("tam. alocado: {}", s.alloc_bytes.map(format_bytes).unwrap_or_else(|| "não disponível neste modo".into()));
                println!("ocorrências: {}", s.issues);
                println!("enumerador:  {} · perfil {} · retomadas {} · hardlinks: {}", s.enumerator, s.profile, s.resumed_count, s.hardlink_policy);
                println!("tempo:       varredura {} ms · finalização {} ms", s.scan_ms.unwrap_or(0), s.finalize_ms.unwrap_or(0));
            });
            Ok(EXIT_OK)
        }
        SizeCmd::Top { snapshot, n } => {
            let rows = snap(&snapshot)?.top_files(n).map_err(|e| e.to_string())?;
            out(json, &rows, |rows| {
                for r in rows {
                    println!("{:>12}  {}", format_bytes(r.size), r.path);
                }
            });
            Ok(EXIT_OK)
        }
        SizeCmd::Dirs { snapshot, n } => {
            let rows = snap(&snapshot)?.top_dirs(n).map_err(|e| e.to_string())?;
            out(json, &rows, |rows| {
                for r in rows {
                    println!("{:>12}  {:>10} arq  {}", format_bytes(r.size), format_count(r.files), r.path.clone().unwrap_or_default());
                }
            });
            Ok(EXIT_OK)
        }
        SizeCmd::Ls { snapshot, dir, offset, limit } => {
            let s = snap(&snapshot)?;
            let rows = s.list_dir(dir, offset, limit).map_err(|e| e.to_string())?;
            out(json, &rows, |rows| {
                println!("{}", s.dir_path(dir).unwrap_or_default());
                for r in rows {
                    println!("{:>12}  {} {}{}", format_bytes(r.size), if r.is_dir { "[D]" } else { "   " }, r.name, if r.is_dir { format!("  (id {})", r.id) } else { String::new() });
                }
            });
            Ok(EXIT_OK)
        }
        SizeCmd::Query(q) => {
            let s = snap(&q.snapshot)?;
            let before = q.before_year.map(|y| days_from_year(y) * 86_400);
            let fq = FileQuery {
                min_size: q.min_size,
                max_size: q.max_size,
                ext: q.ext.clone(),
                modified_before: before,
                year: q.year,
                name_contains: q.name.clone(),
                sort_by_size_desc: true,
                offset: q.offset,
                limit: q.limit,
                time_limit: Some(Duration::from_secs(q.timeout_s)),
                ..Default::default()
            };
            let t = Instant::now();
            let rows = s.query_files(&fq).map_err(|e| e.to_string())?;
            out(json, &rows, |rows| {
                for r in rows {
                    println!("{:>12}  {}", format_bytes(r.size), r.path);
                }
                eprintln!("{} resultados (página offset={}, limite={}) em {} ms", rows.len(), q.offset, q.limit, t.elapsed().as_millis());
            });
            Ok(EXIT_OK)
        }
        SizeCmd::Stats { snapshot, n } => {
            let s = snap(&snapshot)?;
            let ext = s.ext_stats(n).map_err(|e| e.to_string())?;
            let years = s.year_stats().map_err(|e| e.to_string())?;
            out(json, &serde_json::json!({ "extensions": ext, "years": years }), |_| {
                println!("por extensão:");
                for e in &ext {
                    println!("  {:>12}  {:>10} arq  .{}", format_bytes(e.size), format_count(e.files), e.ext);
                }
                println!("por ano (modificação):");
                for y in &years {
                    println!("  {}  {:>12}  {:>10} arq", y.year.map(|v| v.to_string()).unwrap_or_else(|| "(sem data)".into()), format_bytes(y.size), format_count(y.files));
                }
            });
            Ok(EXIT_OK)
        }
        SizeCmd::Issues { snapshot, limit } => {
            let rows = snap(&snapshot)?.issues(0, limit).map_err(|e| e.to_string())?;
            out(json, &rows, |rows| {
                for r in rows {
                    println!("[{}] {} ({:?}) {} — {}", r.severity, r.kind, r.os_code, r.path, r.message);
                }
            });
            Ok(EXIT_OK)
        }
        SizeCmd::Check { snapshot } => {
            let problems = snap(&snapshot)?.verify().map_err(|e| e.to_string())?;
            out(json, &problems, |p| {
                if p.is_empty() {
                    println!("snapshot íntegro");
                } else {
                    for x in p {
                        println!("PROBLEMA: {x}");
                    }
                }
            });
            Ok(if problems.is_empty() { EXIT_OK } else { EXIT_FAIL })
        }
        SizeCmd::Gen { dir, files, per_dir, fanout, max_size, seed } => {
            let t = Instant::now();
            let r = gen::generate(&dir, &gen::GenSpec { files, files_per_dir: per_dir.max(1), fanout: fanout.max(1), max_size, seed }, |n| eprint!("\r{n} arquivos…"))
                .map_err(|e| e.to_string())?;
            eprintln!();
            let v = serde_json::json!({ "files": r.files, "dirs": r.dirs, "bytes": r.bytes, "elapsed_ms": t.elapsed().as_millis() as u64 });
            println!("{v}");
            Ok(EXIT_OK)
        }
        SizeCmd::Bench(b) => bench(b),
    }
}

fn days_from_year(y: i32) -> i64 {
    // days since epoch for Jan 1 of year y
    let y = y as i64 - 1;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = 306; // Mar 1 → Jan 1 of next year offset (153*10+2)/5 + 0
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[derive(serde::Serialize)]
struct BenchResult {
    label: String,
    root: String,
    backend: String,
    profile: String,
    threads: usize,
    persist: bool,
    run: u32,
    files: u64,
    dirs: u64,
    bytes: u64,
    issues: u64,
    scan_ms: u64,
    finalize_ms: u64,
    files_per_s: f64,
    dirs_per_s: f64,
    peak_working_set_mb: f64,
    peak_private_mb: f64,
    cpu_user_ms: u64,
    cpu_kernel_ms: u64,
    cpu_avg_cores: f64,
    io_read_mb: f64,
    io_write_mb: f64,
    index_mb: Option<f64>,
    open_summary_ms: Option<f64>,
    top100_ms: Option<f64>,
    query_ms: Option<f64>,
    ls_root_ms: Option<f64>,
    max_pending_dirs: u64,
    max_spilled_dirs: u64,
    queue_budget_mb: f64,
    cores: usize,
    cpu: String,
    os: String,
    timestamp: i64,
}

fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn bench(b: BenchArgs) -> Result<i32, String> {
    let root = abs(&b.root)?;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let profile: Profile = b.profile.into();
    let backend: Backend = b.backend.into();
    let mut tuning = Tuning::for_profile(profile, nexttool_core::fs::is_remote_path(&root), cores);
    if let Some(t) = b.threads {
        tuning.threads = t.clamp(1, 64);
    }
    let db_dir = b.db_dir.clone().unwrap_or_else(std::env::temp_dir);
    for run in 1..=b.repeat.max(1) {
        let before = nexttool_core::process::self_stats();
        let opts = ScanOptions { root: root.clone(), profile, backend, tuning: tuning.clone(), cross_mounts: false, spill_dir: Some(db_dir.clone()) };
        let enumerator = make_enumerator(backend);
        let progress = Arc::new(ScanProgress::default());
        let db = db_dir.join(format!("bench-{}-{run}.nxs", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
        }
        let sink: Box<dyn ScanSink> = if b.no_persist {
            Box::new(NullSink)
        } else {
            Box::new(IndexWriter::create(&db, &opts, enumerator.name(), progress.clone()).map_err(|e| e.to_string())?)
        };
        let control = Arc::new(ScanControl::new(tuning.threads));
        let (p2, c2, o2, e2) = (progress.clone(), control.clone(), opts.clone(), enumerator.clone());
        let t0 = Instant::now();
        let h = std::thread::spawn(move || run_scan(&o2, e2, sink, ScanStart::Fresh, c2, p2));
        let (mut maxp, mut maxs) = (0, 0);
        while !h.is_finished() {
            let s = progress.snapshot();
            maxp = maxp.max(s.pending_dirs);
            maxs = maxs.max(s.spilled_dirs);
            eprint!("\r[{run}] {} arq | {} dirs | pend {} | spill {} | RAM {}   ", format_count(s.files), format_count(s.dirs), s.pending_dirs, s.spilled_dirs, format_bytes(nexttool_core::process::self_stats().working_set));
            std::thread::sleep(Duration::from_millis(200));
        }
        let report = h.join().map_err(|_| "pânico no motor".to_string())?;
        let wall = t0.elapsed().as_secs_f64();
        eprintln!();
        if let Some(e) = &report.fatal_error {
            return Err(format!("falha: {e}"));
        }
        let after = nexttool_core::process::self_stats();
        let (mut index_mb, mut open_ms, mut top_ms, mut q_ms, mut ls_ms) = (None, None, None, None, None);
        if !b.no_persist {
            index_mb = Some(file_size(&db) as f64 / 1_048_576.0);
            let t = Instant::now();
            let s = Snapshot::open(&db).map_err(|e| e.to_string())?;
            let _ = s.summary().map_err(|e| e.to_string())?;
            open_ms = Some(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let _ = s.top_files(100).map_err(|e| e.to_string())?;
            top_ms = Some(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let _ = s
                .query_files(&FileQuery { ext: Some("bak".into()), min_size: Some(100), sort_by_size_desc: true, limit: 100, time_limit: Some(Duration::from_secs(60)), ..Default::default() })
                .map_err(|e| e.to_string())?;
            q_ms = Some(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let _ = s.list_dir(1, 0, 100).map_err(|e| e.to_string())?;
            ls_ms = Some(t.elapsed().as_secs_f64() * 1e3);
            drop(s);
            if !b.keep_db {
                for ext in ["", "-wal", "-shm"] {
                    let _ = std::fs::remove_file(format!("{}{ext}", db.display()));
                }
            }
        }
        let o = &report.outcome;
        let scan_s = (report.scan_ms as f64 / 1e3).max(1e-3);
        let cpu_ms = (after.user_cpu_ms + after.kernel_cpu_ms).saturating_sub(before.user_cpu_ms + before.kernel_cpu_ms);
        let r = BenchResult {
            label: b.label.clone(),
            root: root.display().to_string(),
            backend: enumerator.name().into(),
            profile: format!("{profile:?}").to_lowercase(),
            threads: tuning.threads,
            persist: !b.no_persist,
            run,
            files: o.files,
            dirs: o.dirs,
            bytes: o.bytes,
            issues: o.issues,
            scan_ms: report.scan_ms,
            finalize_ms: report.finish_ms,
            files_per_s: o.files as f64 / scan_s,
            dirs_per_s: o.dirs as f64 / scan_s,
            peak_working_set_mb: after.peak_working_set as f64 / 1_048_576.0,
            peak_private_mb: after.peak_private_bytes as f64 / 1_048_576.0,
            cpu_user_ms: after.user_cpu_ms.saturating_sub(before.user_cpu_ms),
            cpu_kernel_ms: after.kernel_cpu_ms.saturating_sub(before.kernel_cpu_ms),
            cpu_avg_cores: cpu_ms as f64 / 1e3 / wall,
            io_read_mb: after.io_read_bytes.saturating_sub(before.io_read_bytes) as f64 / 1_048_576.0,
            io_write_mb: after.io_write_bytes.saturating_sub(before.io_write_bytes) as f64 / 1_048_576.0,
            index_mb,
            open_summary_ms: open_ms,
            top100_ms: top_ms,
            query_ms: q_ms,
            ls_root_ms: ls_ms,
            max_pending_dirs: maxp,
            max_spilled_dirs: maxs,
            queue_budget_mb: tuning.queue_budget_bytes() as f64 / 1_048_576.0,
            cores,
            cpu: std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),
            os: std::env::consts::OS.into(),
            timestamp: nexttool_core::time::now_unix(),
        };
        let line = serde_json::to_string(&r).unwrap_or_default();
        println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
        if let Some(p) = &b.json_out {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| e.to_string())?;
            writeln!(f, "{line}").map_err(|e| e.to_string())?;
        }
    }
    Ok(EXIT_OK)
}

fn copy(c: CopyCmd, data_dir: &Path, json: bool) -> Result<i32, String> {
    match c {
        CopyCmd::Plan(a) => {
            let mut job = a.job()?;
            job.source = abs(&job.source)?;
            job.dest = abs(&job.dest)?;
            #[cfg(windows)]
            {
                use nextcopy_core::CopyEngine;
                let eng = nextcopy_robocopy::RobocopyEngine::locate().map_err(|e| e.to_string())?;
                let rep = eng
                    .dry_run(&job, &data_dir.join("logs").join("copy").join("plans"), &nextcopy_core::CopyControl::default())
                    .map_err(|e| e.to_string())?;
                out(json, &rep, |r| {
                    println!("prévia (nada foi alterado):");
                    println!("  seriam copiados: {} arquivos, {}", format_count(r.would_copy_files), format_bytes(r.would_copy_bytes));
                    if job.options.mirror {
                        println!("  seriam EXCLUÍDOS no destino: {} arquivos e {} diretórios", r.would_delete_files, r.would_delete_dirs);
                        for p in r.delete_sample.iter().take(20) {
                            println!("    - {p}");
                        }
                        println!("  para executar: nexttool-cli copy run ... --profile mirror --approve-deletions {}", r.would_delete_files + r.would_delete_dirs);
                    }
                    println!("  erros na prévia: {} · log: {}", r.errors, r.log_file);
                });
                Ok(EXIT_OK)
            }
            #[cfg(not(windows))]
            {
                let _ = (data_dir, json, job);
                Err("NextCopy v1 requer Windows (Robocopy)".into())
            }
        }
        CopyCmd::Run(a) => {
            let mut job = a.job()?;
            job.source = abs(&job.source)?;
            job.dest = abs(&job.dest)?;
            if job.options.mirror && job.approved_deletions.is_none() {
                return Err("espelhamento exclui arquivos no destino: execute `nexttool-cli copy plan` e informe --approve-deletions <N>".into());
            }
            let title = format!("Cópia {} → {}", job.source.display(), job.dest.display());
            run_task(data_dir, TaskSpec::Copy(CopySpec { job }), title, json)
        }
        CopyCmd::Verify { src, dst, strict } => {
            let mut job = CopyJob::new(abs(&src)?, abs(&dst)?, CopyProfile::Safe);
            job.validate().map_err(|e| e.to_string())?;
            let level = if strict { VerifyLevel::Strict } else { VerifyLevel::Standard };
            job.options.verify = level;
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let en = make_enumerator(Backend::Auto);
            let rep = nextcopy_verify::verify(&job, level, en.as_ref(), &cancel, &mut |_| {});
            out(json, &rep, |r| {
                println!(
                    "{}: {} conferidos · {} ok · ausentes {} · tamanho {} · data {} · hash {} · erros {} · {} ms",
                    if r.passed() { "APROVADO" } else { "DIVERGÊNCIAS" },
                    r.files_checked, r.files_ok, r.missing, r.size_mismatch, r.time_mismatch, r.hash_mismatch, r.errors, r.elapsed_ms
                );
                for d in r.sample.iter().take(20) {
                    println!("  {:?} {} {}", d.kind, d.rel_path, d.detail);
                }
            });
            Ok(if rep.passed() { EXIT_OK } else { EXIT_FAIL })
        }
    }
}

fn tasks(c: TasksCmd, data_dir: &Path, json: bool) -> Result<i32, String> {
    match c {
        TasksCmd::List { limit } => {
            let s = open_store(data_dir)?;
            let rows = s.list_tasks(None, limit, 0).map_err(|e| e.to_string())?;
            out(json, &rows, |rows| {
                for r in rows {
                    println!("{:>5}  {:<24} {:<9} {}", r.id, r.state.as_str(), r.kind.as_str(), r.title);
                }
            });
            Ok(EXIT_OK)
        }
        TasksCmd::Show { id } => {
            let s = open_store(data_dir)?;
            let r = s.get_task(id).map_err(|e| e.to_string())?;
            let ev = s.events(id, 30).map_err(|e| e.to_string())?;
            out(json, &serde_json::json!({ "task": r, "events": ev }), |_| {
                print_task(&r);
                for (at, lvl, msg) in ev.iter().rev() {
                    println!("  {at} [{lvl}] {msg}");
                }
            });
            Ok(EXIT_OK)
        }
        TasksCmd::Resume { id } => {
            let mgr = manager(data_dir)?;
            install_ctrlc(&mgr);
            mgr.resume(id).map_err(|e| e.to_string())?;
            let r = mgr.run_until_done(&[id], &mut |_| {}).map_err(|e| e.to_string())?;
            print_task(&r[0]);
            Ok(exit_for(r[0].state))
        }
        TasksCmd::Cancel { id } => {
            let mgr = manager(data_dir)?;
            let ok = mgr.cancel(id).map_err(|e| e.to_string())?;
            println!("{}", if ok { "cancelada" } else { "não foi possível cancelar (estado terminal)" });
            Ok(EXIT_OK)
        }
    }
}

fn alerts(c: AlertsCmd, data_dir: &Path, json: bool) -> Result<i32, String> {
    let s = open_store(data_dir)?;
    match c {
        AlertsCmd::List { all } => {
            let rows = s.list_alerts(all, 100).map_err(|e| e.to_string())?;
            out(json, &rows, |rows| {
                for a in rows {
                    let d = &a.draft;
                    println!("#{} [{}] {} — {} ({}×)", a.id, d.severity.as_str(), d.title, d.module.as_str(), a.count);
                    println!("    problema: {}", d.problem);
                    if let Some(c) = &d.probable_cause {
                        println!("    causa provável: {c}");
                    }
                    println!("    consequência: {}", d.consequence);
                    println!("    ação: {}", d.recommended_action);
                    if let Some(x) = &d.details {
                        println!("    detalhes: {x}");
                    }
                }
            });
        }
        AlertsCmd::Ack { id } => {
            s.ack_alert(id).map_err(|e| e.to_string())?;
        }
    }
    Ok(EXIT_OK)
}
