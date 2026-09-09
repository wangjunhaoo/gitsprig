use crate::{
    git::{git_executable, shell_quote, Context, Result},
    model::Prompt,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct HelperRequest {
    token: String,
    kind: String,
    content: String,
}
#[derive(Serialize, Deserialize)]
struct HelperResponse {
    value: Option<String>,
    error: Option<String>,
}
pub type Ask = Arc<dyn Fn(Prompt, Arc<AtomicBool>) -> Result<String> + Send + Sync>;

pub struct Bridge {
    stop: Arc<AtomicBool>,
    pub env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
}

impl Bridge {
    pub fn start(operation_id: String, context: &Context, ask: Ask) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let token = uuid::Uuid::new_v4().to_string();
        let expected_token = token.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let cancel = context.cancel.clone();
        thread::spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) && !cancel.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        let mut line = String::new();
                        let read = BufReader::new(&mut stream)
                            .take(1024 * 1024)
                            .read_line(&mut line);
                        let request = read
                            .ok()
                            .and_then(|_| serde_json::from_str::<HelperRequest>(&line).ok());
                        if let Some(request) = request {
                            if request.token != expected_token {
                                continue;
                            }
                            let prompt = Prompt {
                                id: uuid::Uuid::new_v4().to_string(),
                                operation_id: operation_id.clone(),
                                title: match request.kind.as_str() {
                                    "sequence" => "编辑变基提交序列",
                                    "editor" => "编辑提交说明",
                                    _ => "Git 身份验证",
                                }
                                .into(),
                                kind: request.kind,
                                content: request.content,
                            };
                            let response = match ask(prompt, cancel.clone()) {
                                Ok(value) => HelperResponse {
                                    value: Some(value),
                                    error: None,
                                },
                                Err(error) => HelperResponse {
                                    value: None,
                                    error: Some(error),
                                },
                            };
                            if let Ok(mut bytes) = serde_json::to_vec(&response) {
                                bytes.push(b'\n');
                                let _ = stream.write_all(&bytes);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(30))
                    }
                    Err(_) => break,
                }
            }
        });
        let executable = shell_quote(context.helper.as_os_str());
        Ok(Self {
            stop,
            env: vec![
                ("GITGUI_BRIDGE".into(), address.to_string().into()),
                ("GITGUI_TOKEN".into(), token.into()),
                (
                    "GIT_EDITOR".into(),
                    format!("{executable} --gitgui-helper editor").into(),
                ),
                (
                    "GIT_SEQUENCE_EDITOR".into(),
                    format!("{executable} --gitgui-helper sequence").into(),
                ),
                ("GIT_ASKPASS".into(), context.helper.as_os_str().to_owned()),
                ("SSH_ASKPASS".into(), context.helper.as_os_str().to_owned()),
                ("SSH_ASKPASS_REQUIRE".into(), "prefer".into()),
                ("DISPLAY".into(), "gitgui:0".into()),
            ],
        })
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn entry() -> bool {
    let mut args: Vec<_> = std::env::args_os().collect();
    // ASKPASS 只接受可执行文件路径，提示文本作为唯一参数传入。
    if args.len() == 2
        && args[1] != "--gitgui-helper"
        && args[1] != "--gitgui-hook"
        && std::env::var_os("GITGUI_BRIDGE").is_some()
        && std::env::var_os("GITGUI_TOKEN").is_some()
    {
        args.insert(1, "--gitgui-helper".into());
        args.insert(2, "askpass".into());
    }
    if args
        .get(1)
        .is_none_or(|a| a != "--gitgui-helper" && a != "--gitgui-hook")
    {
        return false;
    }
    let result = if args[1] == "--gitgui-hook" {
        run_hook(&args)
    } else {
        run_helper(&args)
    };
    match result {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run_helper(args: &[std::ffi::OsString]) -> Result<()> {
    let kind = args
        .get(2)
        .and_then(|s| s.to_str())
        .ok_or("缺少编辑器类型")?;
    let value = args.get(3).ok_or("缺少 Git 编辑器参数")?;
    let content = if kind == "askpass" {
        value.to_string_lossy().into_owned()
    } else {
        fs::read_to_string(value).map_err(|e| format!("无法读取 Git 编辑内容：{e}"))?
    };
    let address = std::env::var("GITGUI_BRIDGE").map_err(|_| "GitGUI 编辑器连接已关闭")?;
    let token = std::env::var("GITGUI_TOKEN").map_err(|_| "缺少 GitGUI 编辑器令牌")?;
    let mut stream = TcpStream::connect(address).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(1800)))
        .map_err(|e| e.to_string())?;
    let mut bytes = serde_json::to_vec(&HelperRequest {
        token,
        kind: kind.into(),
        content,
    })
    .map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|e| e.to_string())?;
    let response: HelperResponse =
        serde_json::from_str(&response).map_err(|_| "编辑已取消或应用已退出")?;
    let value = response
        .value
        .ok_or_else(|| response.error.unwrap_or_else(|| "编辑已取消".into()))?;
    if kind == "askpass" {
        println!("{value}");
    } else {
        fs::write(args.get(3).ok_or("编辑文件路径丢失")?, value).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn run_hook(args: &[std::ffi::OsString]) -> Result<()> {
    let name = args.get(2).ok_or("缺少钩子名称")?;
    let original =
        PathBuf::from(std::env::var_os("GITGUI_ORIGINAL_HOOKS").ok_or("缺少原钩子目录")?)
            .join(name);
    if executable(&original) {
        let status = Command::new(&original)
            .args(&args[3..])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("仓库钩子 {} 未通过", name.to_string_lossy()));
        }
    }
    // 在最后一个提交前钩子之后校验索引，阻止钩子扩大用户选择范围。
    if name != "post-commit" {
        let expected = std::env::var("GITGUI_EXPECTED_TREE").map_err(|_| "缺少提交树校验值")?;
        let actual = Command::new(git_executable())
            .args(["write-tree"])
            .output()
            .map_err(|e| e.to_string())?;
        if !actual.status.success() || String::from_utf8_lossy(&actual.stdout).trim() != expected {
            return Err("钩子改变了本次选中的提交内容，提交已停止。请检查工作区并重新选择；原暂存内容已保留。".into());
        }
    }
    Ok(())
}

fn executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

pub fn hook_wrappers(directory: &Path, executable: &Path) -> Result<()> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for hook in [
        "pre-commit",
        "prepare-commit-msg",
        "commit-msg",
        "post-commit",
        "post-rewrite",
    ] {
        let path = directory.join(hook);
        let script = format!(
            "#!/bin/sh\nexec {} --gitgui-hook {} \"$@\"\n",
            shell_quote(executable.as_os_str()),
            hook
        );
        fs::write(&path, script).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

use std::io::Read;
