use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, String>;
pub const MAX_OUTPUT: usize = 64 * 1024 * 1024;
pub type Progress = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct Context {
    pub cancel: Arc<AtomicBool>,
    pub progress: Progress,
    pub env: Vec<(OsString, OsString)>,
    pub helper: PathBuf,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(|_| {}),
            env: vec![],
            helper: std::env::current_exe().unwrap_or_default(),
        }
    }
}

#[derive(Clone)]
pub struct Git {
    pub root: PathBuf,
    pub context: Context,
    pub extra_env: Vec<(OsString, OsString)>,
}

pub struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Git {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_owned(),
            context: Context::default(),
            extra_env: vec![],
        }
    }

    pub fn with_context(mut self, context: Context) -> Self {
        self.context = context;
        self
    }

    pub fn index(&self, path: &Path) -> Self {
        let mut git = self.clone();
        git.extra_env
            .push(("GIT_INDEX_FILE".into(), path.as_os_str().to_owned()));
        git
    }

    pub fn raw<I, S>(&self, args: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if self.context.cancel.load(Ordering::SeqCst) {
            return Err("操作已取消".into());
        }
        let mut args: Vec<OsString> = args.into_iter().map(|s| s.as_ref().to_owned()).collect();
        let mut command_index = 0;
        while args.get(command_index).is_some_and(|v| v == "-c") {
            command_index += 2;
        }
        let pathspec_command = args
            .get(command_index)
            .and_then(|v| v.to_str())
            .is_some_and(|v| {
                [
                    "add",
                    "restore",
                    "diff",
                    "ls-files",
                    "ls-tree",
                    "blame",
                    "check-attr",
                    "rm",
                    "log",
                    "checkout",
                ]
                .contains(&v)
            });
        if pathspec_command {
            if let Some(separator) = args.iter().position(|arg| arg == "--") {
                for path in args.iter_mut().skip(separator + 1) {
                    let mut literal = OsString::from(":(literal)");
                    literal.push(&*path);
                    *path = literal;
                }
            }
        }
        let mut command = Command::new(git_executable());
        command
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(&self.root)
            .args(args);
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_LITERAL_PATHSPECS",
            "GIT_GLOB_PATHSPECS",
            "GIT_NOGLOB_PATHSPECS",
            "GIT_ICASE_PATHSPECS",
        ] {
            command.env_remove(key);
        }
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("LC_ALL", "C.UTF-8");
        command
            .envs(self.context.env.iter().cloned())
            .envs(self.extra_env.iter().cloned());
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|e| format!("无法启动 Git：{e}"))?;
        let stdout = child.stdout.take().ok_or("无法读取 Git 输出")?;
        let stderr = child.stderr.take().ok_or("无法读取 Git 错误输出")?;
        let progress = self.context.progress.clone();
        let out = thread::spawn(move || collect(stdout, None));
        let err = thread::spawn(move || collect(stderr, Some(progress)));
        let started = Instant::now();
        let mut cancelled_at = None;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            if self.context.cancel.load(Ordering::SeqCst)
                || started.elapsed() > Duration::from_secs(1800)
            {
                if cancelled_at.is_none() {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGTERM);
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = child.kill();
                    }
                    cancelled_at = Some(Instant::now());
                } else if cancelled_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                    let _ = child.kill();
                }
            }
            thread::sleep(Duration::from_millis(15));
        };
        let stdout = out.join().map_err(|_| "Git 输出读取线程异常")??;
        let stderr = err.join().map_err(|_| "Git 错误读取线程异常")??;
        if cancelled_at.is_some() {
            return Err("操作已取消；请查看刷新后的仓库状态，已完成的步骤不会自动撤销".into());
        }
        Ok(Output {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    }

    pub fn run<I, S>(&self, args: I) -> Result<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.raw(args)?;
        if output.code != 0 {
            let message = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Git 操作失败（{}）：{}",
                output.code,
                redact(message.trim())
            ));
        }
        Ok(output.stdout)
    }

    pub fn text<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let bytes = self.run(args)?;
        String::from_utf8(bytes)
            .map(|s| s.trim_end_matches('\n').to_owned())
            .map_err(|_| "Git 返回了无法解码的文本".into())
    }

    pub fn path(&self, name: &str) -> Result<PathBuf> {
        let bytes = self.run(["rev-parse", "--path-format=absolute", "--git-path", name])?;
        Ok(PathBuf::from(bytes_to_os(trim_newline(&bytes))?))
    }
}

fn collect(mut stream: impl Read, progress: Option<Progress>) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut exceeded = false;
    let mut buffer = [0u8; 8192];
    loop {
        let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if let Some(ref callback) = progress {
            callback(&redact(&String::from_utf8_lossy(&buffer[..n])));
        }
        if output.len() + n <= MAX_OUTPUT {
            output.extend_from_slice(&buffer[..n]);
        } else {
            exceeded = true;
        }
    }
    if exceeded {
        Err("Git 输出超过 64 MB，请缩小查询范围".into())
    } else {
        Ok(output)
    }
}

pub fn git_executable() -> OsString {
    if let Some(path) = std::env::var_os("GITGUI_GIT") {
        return path;
    }
    #[cfg(target_os = "macos")]
    {
        for path in [
            "/opt/homebrew/bin/git",
            "/usr/local/bin/git",
            "/usr/bin/git",
        ] {
            if Path::new(path).is_file() {
                return path.into();
            }
        }
    }
    "git".into()
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn trim_newline(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

pub fn bytes_to_os(bytes: &[u8]) -> Result<OsString> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(OsString::from_vec(bytes.to_vec()))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes.to_vec())
            .map(OsString::from)
            .map_err(|_| "文件路径不是有效 UTF-8".into())
    }
}

pub fn os_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        value.to_string_lossy().as_bytes().to_vec()
    }
}

pub fn shell_quote(value: &OsStr) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}

pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    let mut offset = 0;
    while let Some(start) = out[offset..].find("://") {
        let start = offset + start + 3;
        let end = out[start..]
            .find(|c: char| c.is_whitespace() || c == '/' || c == '\'' || c == '"')
            .map(|n| start + n)
            .unwrap_or(out.len());
        if let Some(at) = out[start..end].find('@') {
            out.replace_range(start..start + at, "***");
            offset = start + 4;
        } else {
            offset = end;
        }
        if offset >= out.len() {
            break;
        }
    }
    out
}
