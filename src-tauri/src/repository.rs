use crate::{
    git::{self, bytes_to_os, hash, os_bytes, Git, Result},
    model::*,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};
use std::{
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

pub const PREVIEW_LIMIT: u64 = 5 * 1024 * 1024;
pub const FULL_LIMIT: u64 = 32 * 1024 * 1024;

pub fn file_id(path: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(path)
}

pub fn decode_path(id: &str) -> Result<PathBuf> {
    let bytes = URL_SAFE_NO_PAD.decode(id).map_err(|_| "文件标识无效")?;
    let path = PathBuf::from(bytes_to_os(&bytes)?);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("文件路径超出仓库范围".into());
    }
    if path
        .components()
        .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
    {
        return Err("不允许编辑 Git 内部文件".into());
    }
    Ok(path)
}

pub fn safe_work_path(git: &Git, id: &str) -> Result<PathBuf> {
    let relative = decode_path(id)?;
    let absolute = git.root.join(&relative);
    let canonical_root = fs::canonicalize(&git.root).map_err(|e| e.to_string())?;
    let mut parent = absolute.parent();
    while let Some(path) = parent {
        if path == git.root {
            break;
        }
        if path.exists() {
            let resolved = fs::canonicalize(path).map_err(|e| e.to_string())?;
            if !resolved.starts_with(&canonical_root) {
                return Err("路径经过仓库外部的符号链接，无法操作".into());
            }
            break;
        }
        parent = path.parent();
    }
    Ok(absolute)
}

pub fn open(path: &str) -> Result<Repository> {
    let requested = fs::canonicalize(path).map_err(|e| format!("无法打开目录：{e}"))?;
    let git = Git::new(&requested);
    let version = git.text(["--version"])?;
    let numbers: Vec<u32> = version
        .split_whitespace()
        .nth(2)
        .ok_or("无法识别 Git 版本")?
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    if numbers.len() < 2 || (numbers[0], numbers[1]) < (2, 40) {
        return Err("需要 Git 2.40 或更新版本".into());
    }
    if git.text(["rev-parse", "--is-bare-repository"])? == "true" {
        return Err("请选择带工作区的仓库；裸仓库可作为远程使用".into());
    }
    let root = git.run(["rev-parse", "--show-toplevel"])?;
    let root = fs::canonicalize(PathBuf::from(bytes_to_os(git::trim_newline(&root))?))
        .map_err(|e| e.to_string())?;
    let git = Git::new(&root);
    let git_dir = git.path(".")?;
    let common = git.text(["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Ok(Repository {
        id: hash(&os_bytes(root.as_os_str())),
        path: root.to_string_lossy().into_owned(),
        name: root
            .file_name()
            .ok_or("仓库目录名称为空")?
            .to_string_lossy()
            .into_owned(),
        git_dir: git_dir.to_string_lossy().into_owned(),
        common_dir: common,
    })
}

pub fn head(git: &Git) -> Result<Option<String>> {
    let out = git.raw(["rev-parse", "--verify", "HEAD"])?;
    if out.code == 0 {
        Ok(Some(String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    } else if out.code == 128 {
        Ok(None)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

pub fn index_bytes(git: &Git) -> Result<Vec<u8>> {
    match fs::read(git.path("index")?) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.to_string()),
    }
}

pub fn status(git: &Git) -> Result<RepositoryStatus> {
    let output = git.run([
        "status",
        "--porcelain=v2",
        "-z",
        "--branch",
        "--untracked-files=all",
    ])?;
    let mut branch = "未创建分支".to_owned();
    let mut current_head = None;
    let mut upstream = None;
    let mut ahead = 0;
    let mut behind = 0;
    let mut files = vec![];
    let mut records = output.split(|b| *b == 0).filter(|v| !v.is_empty());
    while let Some(record) = records.next() {
        if let Some(value) = record.strip_prefix(b"# branch.head ") {
            branch = String::from_utf8_lossy(value).into_owned();
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if value != b"(initial)" {
                current_head = Some(String::from_utf8_lossy(value).into_owned());
            }
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.upstream ") {
            upstream = Some(String::from_utf8_lossy(value).into_owned());
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.ab ") {
            let parts: Vec<_> = value.split(|b| *b == b' ').collect();
            if parts.len() == 2 {
                ahead = String::from_utf8_lossy(parts[0])
                    .trim_start_matches('+')
                    .parse()
                    .map_err(|_| "上游状态解析失败")?;
                behind = String::from_utf8_lossy(parts[1])
                    .trim_start_matches('-')
                    .parse()
                    .map_err(|_| "上游状态解析失败")?;
            }
            continue;
        }
        if record.starts_with(b"# ") {
            continue;
        }
        let (path, xy, submodule, old_path, untracked, conflict, intent_to_add) = match record[0] {
            b'1' | b'2' => {
                let count = if record[0] == b'1' { 9 } else { 10 };
                let fields: Vec<_> = record.splitn(count, |b| *b == b' ').collect();
                if fields.len() != count {
                    return Err("Git 文件状态格式异常".into());
                }
                let old = if record[0] == b'2' {
                    Some(records.next().ok_or("重命名记录不完整")?.to_vec())
                } else {
                    None
                };
                (
                    fields[count - 1].to_vec(),
                    fields[1].to_vec(),
                    fields[2].starts_with(b"S"),
                    old,
                    false,
                    false,
                    fields[1] == b".A" && fields[3] == b"000000" && fields[4] == b"000000",
                )
            }
            b'u' => {
                let fields: Vec<_> = record.splitn(11, |b| *b == b' ').collect();
                if fields.len() != 11 {
                    return Err("Git 冲突状态格式异常".into());
                }
                (
                    fields[10].to_vec(),
                    fields[1].to_vec(),
                    fields[2].starts_with(b"S"),
                    None,
                    false,
                    true,
                    false,
                )
            }
            b'?' => (
                record[2..].to_vec(),
                b"??".to_vec(),
                false,
                None,
                true,
                false,
                false,
            ),
            b'!' => continue,
            _ => return Err("不支持的 Git 状态记录".into()),
        };
        let id = file_id(&path);
        let size = fs::symlink_metadata(git.root.join(bytes_to_os(&path)?))
            .map(|m| m.len())
            .unwrap_or(0);
        files.push(FileChange {
            id,
            path: String::from_utf8_lossy(&path).into_owned(),
            old_id: old_path.as_ref().map(|p| file_id(p)),
            old_path: old_path.map(|p| String::from_utf8_lossy(&p).into_owned()),
            status: String::from_utf8_lossy(&xy).into_owned(),
            staged: !untracked && xy.first().is_some_and(|b| *b != b'.'),
            unstaged: untracked || xy.get(1).is_some_and(|b| *b != b'.'),
            untracked,
            intent_to_add,
            conflict,
            submodule,
            size,
        });
    }
    let mut fingerprint = Sha256::new();
    fingerprint.update(&output);
    fingerprint.update(index_bytes(git)?);
    for file in &files {
        if let Ok(path) = decode_path(&file.id) {
            if let Ok(meta) = fs::symlink_metadata(git.root.join(path)) {
                fingerprint.update(meta.len().to_le_bytes());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    fingerprint.update(meta.mtime().to_le_bytes());
                    fingerprint.update(meta.mtime_nsec().to_le_bytes());
                    fingerprint.update(meta.ctime().to_le_bytes());
                    fingerprint.update(meta.ctime_nsec().to_le_bytes());
                    fingerprint.update(meta.mode().to_le_bytes());
                }
            }
        }
    }
    let operation = operation_state(git, &files)?;
    let recovery_dir = git.path("gitgui-recovery")?;
    let recovery = fs::read_dir(recovery_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("transaction.json").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    Ok(RepositoryStatus {
        snapshot: format!("{:x}", fingerprint.finalize()),
        head: current_head,
        branch,
        upstream,
        ahead,
        behind,
        files,
        operation,
        recovery,
    })
}

pub fn operation_state(git: &Git, files: &[FileChange]) -> Result<Option<OperationState>> {
    let conflicts = files.iter().filter(|f| f.conflict).count();
    let rebase_merge = git.path("rebase-merge")?;
    let rebase_apply = git.path("rebase-apply")?;
    let operation = if rebase_merge.is_dir() || rebase_apply.is_dir() {
        let dir = if rebase_merge.is_dir() {
            rebase_merge
        } else {
            rebase_apply
        };
        let detail = if conflicts > 0 {
            format!("还有 {conflicts} 个冲突文件")
        } else if dir.join("amend").exists() || dir.join("stopped-sha").exists() {
            "已暂停，可以编辑当前提交后继续".into()
        } else {
            "变基进行中".into()
        };
        Some(OperationState {
            kind: "rebase".into(),
            detail,
            can_continue: conflicts == 0,
            can_skip: true,
            can_abort: true,
        })
    } else if git.path("MERGE_HEAD")?.exists() {
        Some(OperationState {
            kind: "merge".into(),
            detail: format!("合并进行中，{conflicts} 个冲突文件"),
            can_continue: conflicts == 0,
            can_skip: false,
            can_abort: true,
        })
    } else if git.path("CHERRY_PICK_HEAD")?.exists() {
        Some(OperationState {
            kind: "cherry-pick".into(),
            detail: "挑选提交进行中".into(),
            can_continue: conflicts == 0,
            can_skip: true,
            can_abort: true,
        })
    } else if git.path("REVERT_HEAD")?.exists() {
        Some(OperationState {
            kind: "revert".into(),
            detail: "撤销提交进行中".into(),
            can_continue: conflicts == 0,
            can_skip: true,
            can_abort: true,
        })
    } else if git.path("sequencer")?.is_dir() {
        let todo = fs::read_to_string(git.path("sequencer/todo")?).unwrap_or_default();
        let kind = if todo.starts_with("revert ") {
            "revert"
        } else {
            "cherry-pick"
        };
        Some(OperationState {
            kind: kind.into(),
            detail: "提交序列已暂停".into(),
            can_continue: conflicts == 0,
            can_skip: true,
            can_abort: true,
        })
    } else {
        None
    };
    Ok(operation)
}

pub fn resolve_commit(git: &Git, revision: &str) -> Result<String> {
    if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
        return Err("提交引用无效".into());
    }
    git.text([
        "rev-parse",
        "--verify",
        "--end-of-options",
        &format!("{revision}^{{commit}}"),
    ])
}

pub fn blob(git: &Git, revision: &str, path: &Path) -> Result<Vec<u8>> {
    let output = git.run(vec![
        "ls-tree".into(),
        "-z".into(),
        revision.into(),
        "--".into(),
        path.as_os_str().to_owned(),
    ])?;
    if output.is_empty() {
        return Ok(vec![]);
    }
    let tab = output
        .iter()
        .position(|b| *b == b'\t')
        .ok_or("树对象条目无效")?;
    let fields: Vec<_> = output[..tab].split(|b| *b == b' ').collect();
    if fields.len() != 3 {
        return Err("树对象格式无效".into());
    }
    let oid = String::from_utf8_lossy(fields[2]);
    if fields[1] == b"commit" {
        return Ok(format!("Subproject commit {oid}\n").into_bytes());
    }
    git.run(["cat-file", "blob", &oid])
}

pub fn work_content(git: &Git, id: &str, limit: u64) -> Result<Vec<u8>> {
    let path = safe_work_path(git, id)?;
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    if meta.file_type().is_symlink() {
        return fs::read_link(path)
            .map(|p| os_bytes(p.as_os_str()))
            .map_err(|e| e.to_string());
    }
    if meta.is_dir() {
        return Err("该路径是目录或子模块，请打开对应仓库查看".into());
    }
    if meta.len() > limit {
        return Err(format!(
            "文件超过 {} MB，无法在内置编辑器中打开",
            limit / 1024 / 1024
        ));
    }
    fs::read(path).map_err(|e| e.to_string())
}

pub fn make_hunks(old: &str, new: &str) -> Vec<DiffHunk> {
    let mut occurrences = std::collections::HashMap::<String, usize>::new();
    let diff = TextDiff::configure()
        .timeout(std::time::Duration::from_millis(500))
        .diff_lines(old, new);
    diff.grouped_ops(3)
        .iter()
        .map(|group| {
            let mut lines = vec![];
            for op in group {
                for change in diff.iter_changes(op) {
                    let kind = match change.tag() {
                        ChangeTag::Delete => "delete",
                        ChangeTag::Insert => "insert",
                        ChangeTag::Equal => "equal",
                    };
                    let text = change.value().to_owned();
                    let old_line = change.old_index().map(|v| v + 1);
                    let new_line = change.new_index().map(|v| v + 1);
                    let id = hash(format!("{kind}\0{old_line:?}\0{new_line:?}\0{text}").as_bytes());
                    lines.push(DiffLine {
                        id,
                        kind: kind.into(),
                        text,
                        old_line,
                        new_line,
                    });
                }
            }
            // 代码块按内容和上下文标识，前方提交造成的行号移动不会破坏分组。
            let mut fingerprint = Sha256::new();
            for line in &lines {
                fingerprint.update(line.kind.as_bytes());
                fingerprint.update([0]);
                fingerprint.update(line.text.as_bytes());
                fingerprint.update([0]);
            }
            let body = format!("{:x}", fingerprint.finalize());
            let occurrence = occurrences.entry(body.clone()).or_default();
            let id = format!("{body}-{occurrence}");
            *occurrence += 1;
            DiffHunk {
                id,
                old_start: group.first().map(|o| o.old_range().start + 1).unwrap_or(1),
                new_start: group.first().map(|o| o.new_range().start + 1).unwrap_or(1),
                lines,
            }
        })
        .collect()
}

pub fn diff(
    git: &Git,
    id: &str,
    from: Option<&str>,
    to: Option<&str>,
    full: bool,
) -> Result<FileDiff> {
    let relative = decode_path(id)?;
    let state = status(git)?;
    let file = state.files.iter().find(|f| f.id == id);
    let new_revision = to.map(|r| resolve_commit(git, r)).transpose()?;
    let old_revision = if let Some(from) = from {
        Some(resolve_commit(git, from)?)
    } else if let Some(to) = &new_revision {
        let parents = git.text(["rev-list", "--parents", "-n", "1", to])?;
        parents.split_whitespace().nth(1).map(String::from)
    } else {
        state.head.clone()
    };
    let historical_files = if let Some(to) = &new_revision {
        commit_files(git, to, from)?
    } else {
        vec![]
    };
    let source_file = if new_revision.is_some() {
        historical_files.iter().find(|f| f.id == id)
    } else {
        file
    };
    let old_relative = source_file
        .and_then(|f| f.old_id.as_ref())
        .map(|id| decode_path(id))
        .transpose()?
        .unwrap_or(relative.clone());
    let limit = if full { FULL_LIMIT } else { PREVIEW_LIMIT };
    if file.is_some_and(|f| f.submodule) && from.is_none() {
        return Ok(FileDiff {
            file_id: id.into(),
            path: relative.to_string_lossy().into_owned(),
            snapshot: state.snapshot,
            content_hash: String::new(),
            old_text: None,
            new_text: None,
            binary: true,
            too_large: false,
            size: 0,
            hunks: vec![],
            image_old: None,
            image_new: None,
        });
    }
    let size_for = |revision: &str, path: &Path| -> Result<u64> {
        let mut spec = OsString::from(format!("{revision}:"));
        spec.push(path);
        let output = git.raw(vec![OsString::from("cat-file"), OsString::from("-s"), spec])?;
        if output.code != 0 {
            return Ok(0);
        }
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .map_err(|_| "对象大小无效".into())
    };
    let old_size = old_revision
        .as_ref()
        .map(|r| size_for(r, &old_relative))
        .transpose()?
        .unwrap_or(0);
    let new_size = match new_revision.as_ref() {
        Some(r) => size_for(r, &relative)?,
        None => fs::symlink_metadata(safe_work_path(git, id)?)
            .map(|m| m.len())
            .unwrap_or(0),
    };
    let mut result = FileDiff {
        file_id: id.into(),
        path: relative.to_string_lossy().into_owned(),
        snapshot: state.snapshot,
        content_hash: String::new(),
        old_text: None,
        new_text: None,
        binary: false,
        too_large: old_size.max(new_size) > limit,
        size: old_size.max(new_size),
        hunks: vec![],
        image_old: None,
        image_new: None,
    };
    if result.too_large {
        return Ok(result);
    }
    let old = old_revision
        .as_ref()
        .map(|r| blob(git, r, &old_relative))
        .transpose()?
        .unwrap_or_default();
    let new = match new_revision.as_ref() {
        Some(r) => blob(git, r, &relative)?,
        None => work_content(git, id, limit)?,
    };
    result.content_hash = hash(&new);
    let binary = old.contains(&0)
        || new.contains(&0)
        || std::str::from_utf8(&old).is_err()
        || std::str::from_utf8(&new).is_err();
    result.binary = binary;
    if binary {
        result.image_old = image_data(&relative, &old);
        result.image_new = image_data(&relative, &new);
    } else {
        let old = String::from_utf8(old).map_err(|e| e.to_string())?;
        let new = String::from_utf8(new).map_err(|e| e.to_string())?;
        result.hunks = make_hunks(&old, &new);
        result.old_text = Some(old);
        result.new_text = Some(new);
    }
    Ok(result)
}

fn image_data(path: &Path, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > PREVIEW_LIMIT as usize {
        return None;
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

pub fn refs(git: &Git) -> Result<Vec<Reference>> {
    let output = git.run([
        "for-each-ref",
        "--sort=-committerdate",
        "--format=%(refname)%00%(objectname)%00%(HEAD)%00%(upstream:short)%00",
        "refs/heads",
        "refs/remotes",
        "refs/tags",
    ])?;
    let mut refs = vec![];
    for row in output.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
        let fields: Vec<_> = row.split(|b| *b == 0).collect();
        if fields.len() < 4 {
            return Err("分支数据不完整".into());
        }
        let full = String::from_utf8_lossy(fields[0]).into_owned();
        let (kind, name) = if let Some(name) = full.strip_prefix("refs/heads/") {
            ("local", name)
        } else if let Some(name) = full.strip_prefix("refs/remotes/") {
            ("remote", name)
        } else {
            (
                "tag",
                full.strip_prefix("refs/tags/").ok_or("引用格式错误")?,
            )
        };
        refs.push(Reference {
            name: name.into(),
            full_name: full,
            oid: String::from_utf8_lossy(fields[1]).into_owned(),
            current: fields[2] == b"*",
            upstream: String::from_utf8_lossy(fields[3]).into_owned(),
            kind: kind.into(),
        });
    }
    Ok(refs)
}

pub fn history(git: &Git, query: &HistoryQuery) -> Result<Vec<Commit>> {
    if head(git)?.is_none() {
        return Ok(vec![]);
    }
    let limit = query.limit.clamp(1, 500);
    let mut args: Vec<OsString> = vec![
        "log".into(),
        "--topo-order".into(),
        "--date-order".into(),
        "--decorate=short".into(),
        "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%s%x00%D%x00".into(),
        format!("--max-count={limit}").into(),
        format!("--skip={}", query.skip).into(),
    ];
    if let Some(ref author) = query.author {
        if !author.is_empty() {
            args.push(format!("--author={author}").into());
        }
    }
    if let Some(ref search) = query.search {
        if !search.is_empty() {
            args.push("--fixed-strings".into());
            args.push("--regexp-ignore-case".into());
            args.push(format!("--grep={search}").into());
        }
    }
    if let Some(ref revision) = query.revision {
        args.push(resolve_commit(git, revision)?.into());
    } else {
        args.push("--all".into());
        args.push("HEAD".into());
    }
    args.push("--".into());
    if let Some(ref path) = query.path {
        if !path.is_empty() {
            args.push(path.into());
        }
    }
    let data = git.run(args)?;
    let mut commits = vec![];
    let fields: Vec<_> = data.split(|b| *b == 0).collect();
    for record in fields.chunks(7) {
        if record.len() != 7 {
            break;
        }
        let oid = String::from_utf8_lossy(record[0]).trim().to_owned();
        if oid.is_empty() {
            continue;
        }
        commits.push(Commit {
            oid,
            parents: String::from_utf8_lossy(record[1])
                .split_whitespace()
                .map(String::from)
                .collect(),
            author: String::from_utf8_lossy(record[2]).into_owned(),
            email: String::from_utf8_lossy(record[3]).into_owned(),
            timestamp: String::from_utf8_lossy(record[4])
                .parse()
                .map_err(|_| "提交时间无效")?,
            subject: String::from_utf8_lossy(record[5]).into_owned(),
            decorations: String::from_utf8_lossy(record[6]).into_owned(),
        });
    }
    Ok(commits)
}

pub fn commit_files(git: &Git, revision: &str, base: Option<&str>) -> Result<Vec<FileChange>> {
    let oid = resolve_commit(git, revision)?;
    let base = match base {
        Some(b) => Some(resolve_commit(git, b)?),
        None => {
            let parents = git.text(["rev-list", "--parents", "-n", "1", &oid])?;
            parents.split_whitespace().nth(1).map(String::from)
        }
    };
    let output = if let Some(base) = base {
        git.run(["diff", "--name-status", "-z", "-M", &base, &oid, "--"])?
    } else {
        git.run([
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-status",
            "-r",
            "-z",
            "-M",
            &oid,
            "--",
        ])?
    };
    let mut records = output.split(|b| *b == 0).filter(|r| !r.is_empty());
    let mut result = vec![];
    while let Some(status) = records.next() {
        let first = records.next().ok_or("提交文件路径缺失")?;
        let (old, path) = if status.starts_with(b"R") || status.starts_with(b"C") {
            (Some(first), records.next().ok_or("重命名目标缺失")?)
        } else {
            (None, first)
        };
        result.push(FileChange {
            id: file_id(path),
            path: String::from_utf8_lossy(path).into_owned(),
            old_id: old.map(file_id),
            old_path: old.map(|p| String::from_utf8_lossy(p).into_owned()),
            status: String::from_utf8_lossy(status).into_owned(),
            staged: false,
            unstaged: false,
            untracked: false,
            intent_to_add: false,
            conflict: false,
            submodule: false,
            size: 0,
        });
    }
    Ok(result)
}

pub fn remotes(git: &Git) -> Result<Vec<Remote>> {
    let names = git.text(["remote"])?;
    names
        .lines()
        .map(|name| {
            Ok(Remote {
                name: name.into(),
                fetch_url: git.text(["remote", "get-url", "--", name])?,
                push_url: git.text(["remote", "get-url", "--push", "--", name])?,
            })
        })
        .collect()
}

pub fn blame(git: &Git, id: &str, revision: Option<&str>) -> Result<String> {
    let mut args: Vec<OsString> = vec!["blame".into(), "--line-porcelain".into()];
    if let Some(revision) = revision {
        args.push(resolve_commit(git, revision)?.into());
    }
    args.push("--".into());
    args.push(decode_path(id)?.into_os_string());
    git.text(args)
}
