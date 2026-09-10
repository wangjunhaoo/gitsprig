pub mod actions;
pub mod ai;
pub mod commit;
pub mod conflict;
pub mod git;
pub mod helper;
pub mod ignore;
mod menu;
pub mod model;
pub mod push;
pub mod repository;

use git::{Context, Git, Result};
use model::*;
use notify::{RecursiveMode, Watcher};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};

struct WatchState {
    _watcher: notify::RecommendedWatcher,
    stop: Arc<AtomicBool>,
}
impl Drop for WatchState {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub struct AppState {
    repositories: Mutex<HashMap<String, Repository>>,
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    operations: Mutex<HashMap<String, Arc<AtomicBool>>>,
    ai_operations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    ai_settings_gate: Arc<Mutex<()>>,
    prompts: Mutex<HashMap<String, mpsc::Sender<Option<String>>>>,
    watcher: Mutex<Option<WatchState>>,
    data_dir: PathBuf,
    ai_data_dir: PathBuf,
    started: Instant,
}

impl AppState {
    fn repo(&self, id: &str) -> Result<Repository> {
        self.repositories
            .lock()
            .map_err(|_| "仓库状态锁异常")?
            .get(id)
            .cloned()
            .ok_or_else(|| "仓库会话已关闭，请重新打开".into())
    }
    fn gate(&self, key: &str) -> Result<Arc<Mutex<()>>> {
        Ok(self
            .gates
            .lock()
            .map_err(|_| "仓库操作锁异常")?
            .entry(key.into())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|e| format!("后台任务异常：{e}"))?
}

#[tauri::command]
async fn open_repository(path: String, state: State<'_, AppState>) -> Result<Repository> {
    let repo = blocking(move || repository::open(&path)).await?;
    state
        .repositories
        .lock()
        .map_err(|_| "仓库状态锁异常")?
        .insert(repo.id.clone(), repo.clone());
    Ok(repo)
}

#[tauri::command]
async fn repository_status(
    repo_id: String,
    state: State<'_, AppState>,
) -> Result<RepositoryStatus> {
    let repo = state.repo(&repo_id)?;
    blocking(move || repository::status(&Git::new(repo.path))).await
}

#[tauri::command]
async fn file_diff(
    repo_id: String,
    file_id: String,
    from: Option<String>,
    to: Option<String>,
    full: bool,
    state: State<'_, AppState>,
) -> Result<FileDiff> {
    let repo = state.repo(&repo_id)?;
    blocking(move || {
        repository::diff(
            &Git::new(repo.path),
            &file_id,
            from.as_deref(),
            to.as_deref(),
            full,
        )
    })
    .await
}

#[tauri::command]
async fn repository_refs(repo_id: String, state: State<'_, AppState>) -> Result<Vec<Reference>> {
    let repo = state.repo(&repo_id)?;
    blocking(move || repository::refs(&Git::new(repo.path))).await
}

#[tauri::command]
async fn repository_history(
    repo_id: String,
    query: HistoryQuery,
    state: State<'_, AppState>,
) -> Result<Vec<Commit>> {
    let repo = state.repo(&repo_id)?;
    blocking(move || repository::history(&Git::new(repo.path), &query)).await
}

#[tauri::command]
async fn commit_files(
    repo_id: String,
    revision: String,
    base: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<FileChange>> {
    let repo = state.repo(&repo_id)?;
    blocking(move || repository::commit_files(&Git::new(repo.path), &revision, base.as_deref()))
        .await
}

#[tauri::command]
async fn repository_remotes(repo_id: String, state: State<'_, AppState>) -> Result<Vec<Remote>> {
    let repo = state.repo(&repo_id)?;
    blocking(move || repository::remotes(&Git::new(repo.path))).await
}

#[tauri::command]
async fn repository_collection(
    repo_id: String,
    kind: String,
    state: State<'_, AppState>,
) -> Result<Value> {
    let repo = state.repo(&repo_id)?;
    blocking(move || actions::collections(&Git::new(repo.path), &kind)).await
}

#[tauri::command]
async fn file_blame(
    repo_id: String,
    file_id: String,
    revision: Option<String>,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    blocking(move || repository::blame(&Git::new(repo.path), &file_id, revision.as_deref())).await
}

#[tauri::command]
async fn commit_message(
    repo_id: String,
    revision: String,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    blocking(move || {
        let git = Git::new(repo.path);
        let oid = repository::resolve_commit(&git, &revision)?;
        git.text(["log", "-1", "--format=%B", &oid])
    })
    .await
}

#[tauri::command]
async fn commit_template(repo_id: String, state: State<'_, AppState>) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    blocking(move || {
        let git = Git::new(&repo.path);
        let config = git.raw(["config", "--path", "--get", "commit.template"])?;
        if config.code == 1 {
            return Ok(String::new());
        }
        if config.code != 0 {
            return Err("无法读取提交模板设置".into());
        }
        let path = PathBuf::from(git::bytes_to_os(git::trim_newline(&config.stdout))?);
        let path = if path.is_absolute() {
            path
        } else {
            git.root.join(path)
        };
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("提交模板超过 1 MB".into());
        }
        fs::read_to_string(path).map_err(|e| e.to_string())
    })
    .await
}

fn spawn_operation(
    app: tauri::AppHandle,
    repo_id: String,
    root: PathBuf,
    gate_key: String,
    job: impl FnOnce(Git) -> Result<String> + Send + 'static,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    let state = app.state::<AppState>();
    state
        .operations
        .lock()
        .map_err(|_| "操作状态锁异常")?
        .insert(id.clone(), cancel.clone());
    let gate = state.gate(&gate_key)?;
    let operation_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let send = |phase: &str, message: String| {
            let _ = app.emit(
                "git-operation",
                OperationEvent {
                    id: operation_id.clone(),
                    repo_id: repo_id.clone(),
                    state: phase.into(),
                    message,
                },
            );
        };
        send("running", "正在执行…".into());
        let result = (|| {
            let _guard = gate.lock().map_err(|_| "仓库操作锁异常")?;
            let progress_app = app.clone();
            let progress_id = operation_id.clone();
            let progress_repo = repo_id.clone();
            let context = Context {
                cancel: cancel.clone(),
                progress: Arc::new(move |text| {
                    let _ = progress_app.emit(
                        "git-operation",
                        OperationEvent {
                            id: progress_id.clone(),
                            repo_id: progress_repo.clone(),
                            state: "progress".into(),
                            message: text.into(),
                        },
                    );
                }),
                ..Context::default()
            };
            let prompt_app = app.clone();
            let prompt_repository = root
                .file_name()
                .ok_or("仓库目录名称缺失")?
                .to_string_lossy()
                .into_owned();
            let ask: helper::Ask = Arc::new(move |mut prompt, cancel| {
                prompt.title = format!("{} · {}", prompt.title, prompt_repository);
                let (tx, rx) = mpsc::channel();
                let prompt_id = prompt.id.clone();
                prompt_app
                    .state::<AppState>()
                    .prompts
                    .lock()
                    .map_err(|_| "输入状态锁异常")?
                    .insert(prompt_id.clone(), tx);
                prompt_app
                    .emit("git-prompt", &prompt)
                    .map_err(|e| e.to_string())?;
                let answer = loop {
                    if cancel.load(Ordering::SeqCst) {
                        break Err("操作已取消".into());
                    }
                    match rx.recv_timeout(Duration::from_millis(250)) {
                        Ok(Some(value)) => break Ok(value),
                        Ok(None) => break Err("用户取消了输入".into()),
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break Err("输入窗口已关闭".into()),
                    }
                };
                if let Ok(mut prompts) = prompt_app.state::<AppState>().prompts.lock() {
                    prompts.remove(&prompt_id);
                }
                answer
            });
            let bridge = helper::Bridge::start(operation_id.clone(), &context, ask)?;
            let mut context = context;
            context.env.extend(bridge.env.clone());
            job(Git::new(root).with_context(context))
        })();
        match result {
            Ok(message) => send("success", message),
            Err(message) => send(
                if cancel.load(Ordering::SeqCst) {
                    "cancelled"
                } else {
                    "error"
                },
                message,
            ),
        }
        if let Ok(mut operations) = app.state::<AppState>().operations.lock() {
            operations.remove(&operation_id);
        }
        let _ = app.emit("repository-changed", &repo_id);
    });
    Ok(id)
}

#[tauri::command]
fn start_action(
    repo_id: String,
    action: GitAction,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    spawn_operation(
        app,
        repo_id,
        repo.path.into(),
        repo.common_dir,
        move |git| actions::execute(&git, &action),
    )
}

#[tauri::command]
fn start_commit(
    repo_id: String,
    request: CommitRequest,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    spawn_operation(
        app,
        repo_id,
        repo.path.into(),
        repo.common_dir,
        move |git| commit::commit(&git, &request),
    )
}

#[tauri::command]
fn start_push(
    repo_id: String,
    request: push::PushRequest,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    spawn_operation(
        app,
        repo_id,
        repo.path.into(),
        repo.common_dir,
        move |git| push::execute(&git, &request),
    )
}

#[tauri::command]
fn start_push_remote_query(
    repo_id: String,
    remote: String,
    request_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String> {
    uuid::Uuid::parse_str(&request_id).map_err(|_| "远程查询标识无效")?;
    let repo = state.repo(&repo_id)?;
    let result_app = app.clone();
    spawn_operation(
        app,
        format!("push-query:{request_id}"),
        repo.path.into(),
        repo.common_dir,
        move |git| {
            let result = push::remote_info(&git, &remote);
            let reply = match &result {
                Ok(info) => json!({"requestId":request_id,"info":info}),
                Err(error) => json!({"requestId":request_id,"error":error}),
            };
            result_app
                .emit("push-remote-result", reply)
                .map_err(|e| e.to_string())?;
            result.map(|info| {
                format!(
                    "已读取 {} 的 {} 个远程分支",
                    info.remote,
                    info.branches.len()
                )
            })
        },
    )
}

#[tauri::command]
fn create_repository(path: String, url: Option<String>, app: tauri::AppHandle) -> Result<String> {
    let target = PathBuf::from(&path);
    if !target.is_absolute() {
        return Err("请选择绝对目录路径".into());
    }
    let parent = target.parent().ok_or("请选择有效的仓库目录")?.to_owned();
    if !parent.is_dir() {
        return Err("目标父目录不存在".into());
    }
    let key = path.clone();
    spawn_operation(app, format!("create:{path}"), parent, key, move |git| {
        if let Some(url) = url {
            if url.is_empty() || url.starts_with('-') || url.contains('\0') {
                return Err("仓库地址无效".into());
            }
            git.run(vec![
                "clone".into(),
                "--progress".into(),
                "--".into(),
                url.into(),
                target.into_os_string(),
            ])?;
        } else {
            git.run(vec![
                "init".into(),
                "-b".into(),
                "main".into(),
                "--".into(),
                target.into_os_string(),
            ])?;
        }
        Ok(format!("仓库已就绪：{path}"))
    })
}

#[tauri::command]
fn cancel_operation(operation_id: String, state: State<'_, AppState>) -> Result<()> {
    let operations = state.operations.lock().map_err(|_| "操作状态锁异常")?;
    let cancel = operations.get(&operation_id).ok_or("操作已经结束")?;
    cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn answer_prompt(
    prompt_id: String,
    value: Option<String>,
    state: State<'_, AppState>,
) -> Result<()> {
    let sender = state
        .prompts
        .lock()
        .map_err(|_| "输入状态锁异常")?
        .remove(&prompt_id)
        .ok_or("该输入请求已结束")?;
    sender.send(value).map_err(|_| "Git 操作已经结束".into())
}

#[tauri::command]
async fn read_conflict(
    repo_id: String,
    file_id: String,
    state: State<'_, AppState>,
) -> Result<ConflictFile> {
    let repo = state.repo(&repo_id)?;
    blocking(move || conflict::read(&Git::new(repo.path), &file_id)).await
}

#[tauri::command]
async fn save_conflict(
    repo_id: String,
    file_id: String,
    expected_hash: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    let gate = state.gate(&repo.common_dir)?;
    blocking(move || {
        let _guard = gate.lock().map_err(|_| "仓库锁异常")?;
        conflict::save(&Git::new(repo.path), &file_id, &expected_hash, &content)
    })
    .await
}

#[tauri::command]
async fn choose_conflict(
    repo_id: String,
    file_id: String,
    side: String,
    expected_hash: String,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    let gate = state.gate(&repo.common_dir)?;
    blocking(move || {
        let _guard = gate.lock().map_err(|_| "仓库锁异常")?;
        conflict::choose(&Git::new(repo.path), &file_id, &side, &expected_hash)
    })
    .await
}

#[tauri::command]
async fn mark_conflict(
    repo_id: String,
    file_id: String,
    expected_hash: String,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    let gate = state.gate(&repo.common_dir)?;
    blocking(move || {
        let _guard = gate.lock().map_err(|_| "仓库锁异常")?;
        conflict::mark(&Git::new(repo.path), &file_id, &expected_hash)
    })
    .await
}

#[tauri::command]
fn watch_repository(
    repo_id: Option<String>,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<()> {
    let mut slot = state.watcher.lock().map_err(|_| "文件监听锁异常")?;
    *slot = None;
    let Some(repo_id) = repo_id else {
        return Ok(());
    };
    let repo = state.repo(&repo_id)?;
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Ok(event) = result {
            if !event.kind.is_access()
                && !event.paths.iter().all(|p| {
                    p.to_string_lossy().contains("gitgui-commit-")
                        || p.to_string_lossy().contains("gitgui-recovery")
                })
            {
                let _ = sender.send(());
            }
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(Path::new(&repo.path), RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    if !Path::new(&repo.git_dir).starts_with(&repo.path) {
        watcher
            .watch(Path::new(&repo.git_dir), RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;
    }
    if !Path::new(&repo.common_dir).starts_with(&repo.path) && repo.common_dir != repo.git_dir {
        watcher
            .watch(Path::new(&repo.common_dir), RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    std::thread::spawn(move || {
        while !stop_thread.load(Ordering::SeqCst) {
            if receiver.recv_timeout(Duration::from_millis(300)).is_ok() {
                let started = Instant::now();
                while receiver.recv_timeout(Duration::from_millis(300)).is_ok()
                    && started.elapsed() < Duration::from_secs(2)
                {}
                if !stop_thread.load(Ordering::SeqCst) {
                    let _ = app.emit("repository-changed", &repo_id);
                }
            }
        }
    });
    *slot = Some(WatchState {
        _watcher: watcher,
        stop,
    });
    Ok(())
}

#[tauri::command]
fn load_preferences(state: State<'_, AppState>) -> Result<Value> {
    match fs::read(state.data_dir.join("preferences.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("偏好设置损坏：{e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(
            json!({"recent":[],"groups":{},"theme":"dark","layout":{"sidebar":290,"history":340}}),
        ),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn save_preferences(value: Value, state: State<'_, AppState>) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("偏好设置超过 2 MB，请清理旧变更分组".into());
    }
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&state.data_dir).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.persist(state.data_dir.join("preferences.json"))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn mark_ready(repo_id: Option<String>, timing: Value, state: State<'_, AppState>) -> Result<()> {
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let metric = json!({"startupMs":state.started.elapsed().as_millis(),"pid":std::process::id(),"repository":repo_id,"version":env!("CARGO_PKG_VERSION"),"frontend":timing});
    fs::write(
        state.data_dir.join("launch-metrics.json"),
        serde_json::to_vec(&metric).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn load_ai_settings(state: State<'_, AppState>) -> Result<ai::Settings> {
    let dir = state.ai_data_dir.clone();
    blocking(move || ai::load(&dir)).await
}

#[tauri::command]
async fn save_ai_settings(
    request: ai::SaveSettings,
    state: State<'_, AppState>,
) -> Result<ai::Settings> {
    let dir = state.ai_data_dir.clone();
    let gate = state.ai_settings_gate.clone();
    blocking(move || {
        let _guard = gate.lock().map_err(|_| "AI 设置锁异常")?;
        ai::save(&dir, request)
    })
    .await
}

struct AiOperation {
    id: String,
    operations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}
impl Drop for AiOperation {
    fn drop(&mut self) {
        if let Ok(mut operations) = self.operations.lock() {
            operations.remove(&self.id);
        }
    }
}

#[tauri::command]
async fn generate_commit_message(
    repo_id: String,
    request_id: String,
    snapshot: String,
    selections: Vec<Selection>,
    state: State<'_, AppState>,
) -> Result<String> {
    let repo = state.repo(&repo_id)?;
    uuid::Uuid::parse_str(&request_id).map_err(|_| "AI 请求标识无效")?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut operations = state.ai_operations.lock().map_err(|_| "AI 操作锁异常")?;
        if !operations.is_empty() {
            return Err("请先完成或取消当前 AI 生成".into());
        }
        operations.insert(request_id.clone(), cancel.clone());
    }
    let _operation = AiOperation {
        id: request_id,
        operations: state.ai_operations.clone(),
    };
    let dir = state.ai_data_dir.clone();
    let settings_gate = state.ai_settings_gate.clone();
    let git = Git::new(repo.path).with_context(Context {
        cancel: cancel.clone(),
        ..Context::default()
    });
    let check_git = git.clone();
    let check_snapshot = snapshot.clone();
    let (settings, key, input) = blocking(move || {
        let (settings, key) = {
            let _guard = settings_gate.lock().map_err(|_| "AI 设置锁异常")?;
            let (settings, key) = ai::load_for_generation(&dir)?;
            ai::endpoint(&settings.endpoint)?;
            (settings, key)
        };
        let input = ai::prepare(&git, &snapshot, &selections)?;
        Ok((settings, key, input))
    })
    .await?;
    let message = ai::generate(
        &settings,
        key.as_deref(),
        &input,
        cancel.clone(),
        Duration::from_secs(90),
    )
    .await?;
    blocking(move || {
        if repository::status(&check_git)?.snapshot != check_snapshot {
            return Err("生成期间仓库发生变化，结果未填入；请重新选择后生成".into());
        }
        Ok(())
    })
    .await?;
    if cancel.load(Ordering::SeqCst) {
        return Err("AI 生成已取消".into());
    }
    Ok(message)
}

#[tauri::command]
fn cancel_ai_generation(request_id: String, state: State<'_, AppState>) -> Result<()> {
    if let Some(cancel) = state
        .ai_operations
        .lock()
        .map_err(|_| "AI 操作锁异常")?
        .get(&request_id)
    {
        cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

pub fn run() {
    let started = Instant::now();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let override_dir = std::env::var_os("GITGUI_DATA_DIR").map(PathBuf::from);
            let ai_data_dir = match &override_dir {
                Some(dir) => dir.join(".gitgui"),
                None => app.path().home_dir()?.join(".gitgui"),
            };
            let data_dir = override_dir.unwrap_or(app.path().app_data_dir()?);
            app.manage(AppState {
                repositories: Mutex::new(HashMap::new()),
                gates: Mutex::new(HashMap::new()),
                operations: Mutex::new(HashMap::new()),
                ai_operations: Arc::new(Mutex::new(HashMap::new())),
                ai_settings_gate: Arc::new(Mutex::new(())),
                prompts: Mutex::new(HashMap::new()),
                watcher: Mutex::new(None),
                data_dir,
                ai_data_dir,
                started,
            });
            menu::install(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_repository,
            repository_status,
            file_diff,
            repository_refs,
            repository_history,
            commit_files,
            repository_remotes,
            repository_collection,
            file_blame,
            commit_message,
            commit_template,
            start_action,
            start_commit,
            start_push,
            start_push_remote_query,
            create_repository,
            cancel_operation,
            answer_prompt,
            read_conflict,
            save_conflict,
            choose_conflict,
            mark_conflict,
            watch_repository,
            load_preferences,
            save_preferences,
            mark_ready,
            load_ai_settings,
            save_ai_settings,
            generate_commit_message,
            cancel_ai_generation,
        ])
        .run(tauri::generate_context!())
        .expect("GitSprig 启动失败");
}
