use crate::{
    git::{self, hash, Git, Result},
    helper,
    model::*,
    repository::{self, decode_path},
};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

#[derive(Clone)]
struct Entry {
    mode: String,
    oid: String,
    stage: u8,
    path: Vec<u8>,
}

fn entries(git: &Git) -> Result<Vec<Entry>> {
    let data = git.run(["ls-files", "--stage", "-z"])?;
    data.split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(|record| {
            let tab = record
                .iter()
                .position(|b| *b == b'\t')
                .ok_or("索引条目缺少路径")?;
            let fields: Vec<_> = record[..tab].split(|b| *b == b' ').collect();
            if fields.len() != 3 {
                return Err("索引条目格式错误".into());
            }
            Ok(Entry {
                mode: String::from_utf8_lossy(fields[0]).into_owned(),
                oid: String::from_utf8_lossy(fields[1]).into_owned(),
                stage: String::from_utf8_lossy(fields[2])
                    .parse()
                    .map_err(|_| "索引阶段无效")?,
                path: record[tab + 1..].to_vec(),
            })
        })
        .collect()
}

fn set_entry(git: &Git, path: &[u8], value: Option<&Entry>) -> Result<()> {
    let path = git::bytes_to_os(path)?;
    if let Some(entry) = value {
        git.run(vec![
            "update-index".into(),
            "--add".into(),
            "--cacheinfo".into(),
            entry.mode.clone().into(),
            entry.oid.clone().into(),
            path,
        ])?;
    } else {
        git.run(vec![
            "update-index".into(),
            "--force-remove".into(),
            "--".into(),
            path,
        ])?;
    }
    Ok(())
}

pub fn selected_text(old: &str, new: &str, ids: &[String]) -> Result<String> {
    let selected: HashSet<_> = ids.iter().cloned().collect();
    if selected.is_empty() {
        return Err("请至少选择一行改动".into());
    }
    let mut seen = HashSet::new();
    let mut result = String::new();
    let diff = TextDiff::configure()
        .timeout(std::time::Duration::from_millis(500))
        .diff_lines(old, new);
    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Delete => "delete",
            ChangeTag::Insert => "insert",
            ChangeTag::Equal => "equal",
        };
        let id = hash(
            format!(
                "{kind}\0{:?}\0{:?}\0{}",
                change.old_index().map(|v| v + 1),
                change.new_index().map(|v| v + 1),
                change.value()
            )
            .as_bytes(),
        );
        let chosen = selected.contains(&id);
        if chosen {
            seen.insert(id);
        }
        match change.tag() {
            ChangeTag::Equal => result.push_str(change.value()),
            ChangeTag::Delete if !chosen => result.push_str(change.value()),
            ChangeTag::Insert if chosen => result.push_str(change.value()),
            _ => {}
        }
    }
    if seen.len() != selected.len() {
        return Err("所选代码行已经改变，请重新检查差异".into());
    }
    Ok(result)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    old_head: Option<String>,
    expected_tree: String,
    original_index_hash: String,
    result_index_hash: String,
    new_head: Option<String>,
    pid: u32,
    post_hook_index_hash: Option<String>,
    post_hook_conflict: Option<String>,
}

pub(crate) struct IndexLock {
    path: PathBuf,
    index: PathBuf,
    finished: bool,
}
impl IndexLock {
    pub(crate) fn acquire(git: &Git) -> Result<Self> {
        let index = git.path("index")?;
        let path = index.with_extension("lock");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("无法锁定暂存区，可能有其他 Git 操作正在执行：{e}"))?;
        Ok(Self {
            path,
            index,
            finished: false,
        })
    }
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.path)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    }
    pub(crate) fn finish(mut self) -> Result<()> {
        fs::rename(&self.path, &self.index).map_err(|e| e.to_string())?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for IndexLock {
    fn drop(&mut self) {
        if !self.finished {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn temporary_index(
    git: &Git,
    directory: &Path,
    name: &str,
    bytes: Option<&[u8]>,
    base: Option<&str>,
) -> Result<Git> {
    let path = directory.join(name);
    let temporary = git.index(&path);
    match bytes {
        Some(bytes) if !bytes.is_empty() => fs::write(&path, bytes).map_err(|e| e.to_string())?,
        _ => {
            if let Some(base) = base {
                temporary.run(["read-tree", base])?;
            } else {
                temporary.run(["read-tree", "--empty"])?;
            }
        }
    }
    Ok(temporary)
}

fn retained_index(
    git: &Git,
    original: &Git,
    candidate: &Git,
    base: &str,
    directory: &Path,
    original_bytes: &[u8],
    paths: &HashSet<Vec<u8>>,
) -> Result<Vec<u8>> {
    let original_tree = original.text(["write-tree"])?;
    let candidate_tree = candidate.text(["write-tree"])?;
    let merged = git.index(&directory.join("merge-index"));
    merged.run([
        "read-tree",
        "-i",
        "-m",
        "--aggressive",
        base,
        &original_tree,
        &candidate_tree,
    ])?;
    let mut unresolved: HashMap<Vec<u8>, Vec<Entry>> = HashMap::new();
    for entry in entries(&merged)? {
        if entry.stage != 0 {
            unresolved
                .entry(entry.path.clone())
                .or_default()
                .push(entry);
        }
    }
    // read-tree 只处理树级合并；同文件独立修改交给 Git 的文本三方合并。
    for (path, stages) in unresolved {
        let base = stages.iter().find(|e| e.stage == 1);
        let ours = stages.iter().find(|e| e.stage == 2);
        let theirs = stages.iter().find(|e| e.stage == 3);
        let (Some(base), Some(ours), Some(theirs)) = (base, ours, theirs) else {
            return Err(format!(
                "{} 的已暂存内容与本次选择存在新增或删除冲突，请先处理暂存区",
                String::from_utf8_lossy(&path)
            ));
        };
        if ours.mode != theirs.mode || !["100644", "100755"].contains(&ours.mode.as_str()) {
            return Err(format!(
                "{} 的文件类型或权限发生冲突",
                String::from_utf8_lossy(&path)
            ));
        }
        let ours_file = directory.join("merge-ours");
        let base_file = directory.join("merge-base");
        let theirs_file = directory.join("merge-theirs");
        for (entry, file) in [
            (ours, &ours_file),
            (base, &base_file),
            (theirs, &theirs_file),
        ] {
            let bytes = git.run(["cat-file", "blob", &entry.oid])?;
            if bytes.contains(&0) {
                return Err("二进制文件的已暂存版本与选择内容不同，请先处理暂存区".into());
            }
            fs::write(file, bytes).map_err(|e| e.to_string())?;
        }
        let output = git.raw(vec![
            OsString::from("merge-file"),
            "-p".into(),
            ours_file.into_os_string(),
            base_file.into_os_string(),
            theirs_file.into_os_string(),
        ])?;
        if output.code != 0 {
            return Err(format!(
                "{} 的选择与已暂存改动重叠；提交尚未执行，请重新处理暂存区",
                String::from_utf8_lossy(&path)
            ));
        }
        let merged_file = directory.join("merged-content");
        fs::write(&merged_file, output.stdout).map_err(|e| e.to_string())?;
        let oid = git.text(vec![
            "hash-object".into(),
            "-w".into(),
            merged_file.into_os_string(),
        ])?;
        set_entry(
            &merged,
            &path,
            Some(&Entry {
                oid,
                mode: ours.mode.clone(),
                stage: 0,
                path: path.clone(),
            }),
        )?;
    }
    let result = temporary_index(git, directory, "result-index", Some(original_bytes), None)?;
    let merged_entries: HashMap<_, _> = entries(&merged)?
        .into_iter()
        .map(|e| (e.path.clone(), e))
        .collect();
    // 从原索引副本出发，仅更新选中的路径，保留其他路径的 flags 和 ITA 条目。
    for path in paths {
        set_entry(&result, path, merged_entries.get(path))?;
    }
    let original_flags = original.run(["ls-files", "-v", "-z"])?;
    for record in original_flags.split(|b| *b == 0).filter(|r| r.len() > 2) {
        let path = &record[2..];
        if !paths.contains(path) || !merged_entries.contains_key(path) {
            continue;
        }
        let flag = record[0];
        let path = git::bytes_to_os(path)?;
        if flag.is_ascii_lowercase() {
            result.run(vec![
                "update-index".into(),
                "--assume-unchanged".into(),
                "--".into(),
                path.clone(),
            ])?;
        }
        if flag.eq_ignore_ascii_case(&b'S') {
            result.run(vec![
                "update-index".into(),
                "--skip-worktree".into(),
                "--".into(),
                path,
            ])?;
        }
    }
    // 生成独立完整索引，避免持久恢复文件依赖会被 Git 清理的 sharedindex。
    result.run(["update-index", "--no-split-index"])?;
    fs::read(directory.join("result-index")).map_err(|e| e.to_string())
}

pub fn commit(git: &Git, request: &CommitRequest) -> Result<String> {
    if request.message.trim().is_empty() {
        return Err("请填写提交说明".into());
    }
    if request.selections.is_empty() && !request.amend {
        return Err("请至少选择一处改动".into());
    }
    let initial = repository::status(git)?;
    if initial.snapshot != request.snapshot {
        return Err("仓库已变化，请刷新后重新确认提交内容".into());
    }
    if initial.operation.is_some() {
        return Err("请先完成或终止当前合并、变基或挑选操作".into());
    }
    if !initial.recovery.is_empty() {
        return Err("请先处理未完成的提交恢复记录".into());
    }
    if initial.files.iter().any(|f| f.conflict) {
        return Err("仍有未解决的冲突".into());
    }
    if request.amend && initial.head.is_none() {
        return Err("空仓库无法修改上次提交".into());
    }
    let original_bytes = repository::index_bytes(git)?;
    let lock = IndexLock::acquire(git)?;
    if repository::status(git)?.snapshot != initial.snapshot {
        return Err("锁定前仓库发生变化，请重新选择".into());
    }
    let git_dir = git.path(".")?;
    let temp = tempfile::Builder::new()
        .prefix("gitgui-commit-")
        .tempdir_in(&git_dir)
        .map_err(|e| e.to_string())?;
    let original = temporary_index(
        git,
        temp.path(),
        "original-index",
        Some(&original_bytes),
        None,
    )?;
    let base_tree = match &initial.head {
        Some(head) => git.text(["rev-parse", &format!("{head}^{{tree}}")])?,
        None => git.text(["hash-object", "-w", "-t", "tree", "--stdin"])?,
    };
    let candidate = temporary_index(git, temp.path(), "candidate-index", None, Some(&base_tree))?;
    let mut paths = HashSet::new();
    let mut selected_ids = HashSet::new();
    for selection in &request.selections {
        if !selected_ids.insert(&selection.file_id) {
            return Err("重复的提交文件选择".into());
        }
        let file = initial
            .files
            .iter()
            .find(|f| f.id == selection.file_id)
            .ok_or("所选文件已不在改动列表中")?;
        let path = decode_path(&selection.file_id)?;
        let bytes = git::os_bytes(path.as_os_str());
        paths.insert(bytes.clone());
        if let Some(ref old) = file.old_id {
            let old_path = decode_path(old)?;
            paths.insert(git::os_bytes(old_path.as_os_str()));
            if !selection.all {
                return Err("重命名文件请按整个文件提交".into());
            }
            candidate.run(vec![
                "update-index".into(),
                "--force-remove".into(),
                "--".into(),
                old_path.into_os_string(),
            ])?;
        }
        repository::safe_work_path(git, &selection.file_id)?;
        if selection.all {
            if let Some(expected) = &selection.content_hash {
                if !file.submodule
                    && hash(&repository::work_content(
                        git,
                        &selection.file_id,
                        repository::FULL_LIMIT,
                    )?) != *expected
                {
                    return Err(format!("{} 已发生变化，请重新检查", file.path));
                }
            }
            candidate.run(vec![
                "add".into(),
                "-A".into(),
                "--".into(),
                path.into_os_string(),
            ])?;
        } else {
            let detail = repository::diff(git, &selection.file_id, None, None, false)?;
            if detail.binary || detail.too_large || file.submodule {
                return Err("此文件只能整体提交".into());
            }
            if selection.content_hash.as_ref() != Some(&detail.content_hash) {
                return Err("文件内容已变化，请重新选择代码行".into());
            }
            let attrs = git.run(vec![
                "check-attr".into(),
                "-z".into(),
                "filter".into(),
                "working-tree-encoding".into(),
                "--".into(),
                path.clone().into_os_string(),
            ])?;
            let attr_fields: Vec<_> = attrs.split(|b| *b == 0).collect();
            if attr_fields.chunks(3).any(|f| {
                f.len() == 3 && ![b"unspecified".as_slice(), b"unset".as_slice()].contains(&f[2])
            }) {
                return Err("带内容过滤器或自定义编码的文件请整体提交，以保留 Git 转换语义".into());
            }
            let text = selected_text(
                detail.old_text.as_deref().ok_or("缺少原文件内容")?,
                detail.new_text.as_deref().ok_or("缺少工作区内容")?,
                &selection.line_ids,
            )?;
            let selected_file = temp.path().join("selected-content");
            fs::write(&selected_file, text).map_err(|e| e.to_string())?;
            let oid = git.text(vec![
                "hash-object".into(),
                "-w".into(),
                "--path".into(),
                path.clone().into_os_string(),
                "--".into(),
                selected_file.into_os_string(),
            ])?;
            let mut mode = entries(&candidate)?
                .iter()
                .find(|e| e.path == bytes)
                .map(|e| e.mode.clone())
                .unwrap_or_else(|| "100644".into());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::symlink_metadata(git.root.join(&path)) {
                    if meta.file_type().is_symlink() {
                        return Err("符号链接只能整体提交".into());
                    }
                    if meta.permissions().mode() & 0o111 != 0 {
                        mode = "100755".into();
                    }
                }
            }
            set_entry(
                &candidate,
                &bytes,
                Some(&Entry {
                    mode,
                    oid,
                    stage: 0,
                    path: bytes.clone(),
                }),
            )?;
        }
    }
    let candidate_tree = candidate.text(["write-tree"])?;
    if candidate_tree == base_tree {
        if !request.amend {
            return Err("所选内容没有形成可提交的改动".into());
        }
        let previous_message = git.text(["log", "-1", "--format=%B", "HEAD"])?;
        if previous_message.trim_end() == request.message.trim_end() {
            return Err("提交内容和说明均未变化".into());
        }
    }
    let retained = retained_index(
        git,
        &original,
        &candidate,
        &base_tree,
        temp.path(),
        &original_bytes,
        &paths,
    )?;
    let expected_worktree = repository::status(git)?.snapshot;
    if expected_worktree != initial.snapshot {
        return Err("准备提交时工作区发生变化，请重新选择".into());
    }
    let hooks = temp.path().join("hooks");
    helper::hook_wrappers(&hooks, &git.context.helper)?;
    let original_hooks = {
        let config = git.raw(["config", "--path", "--get", "core.hooksPath"])?;
        if config.code == 0 {
            let path = PathBuf::from(git::bytes_to_os(git::trim_newline(&config.stdout))?);
            if path.is_absolute() {
                path
            } else {
                git.root.join(path)
            }
        } else if config.code == 1 {
            git.path("hooks")?
        } else {
            return Err("无法读取 Git 钩子配置".into());
        }
    };
    let mut committing = candidate.clone();
    committing.extra_env.push((
        "GITGUI_ORIGINAL_HOOKS".into(),
        original_hooks.into_os_string(),
    ));
    committing
        .extra_env
        .push(("GITGUI_EXPECTED_TREE".into(), candidate_tree.clone().into()));
    let message = temp.path().join("message");
    fs::write(&message, &request.message).map_err(|e| e.to_string())?;
    let transaction_id = uuid::Uuid::new_v4().to_string();
    let transaction_dir = git.path("gitgui-recovery")?.join(&transaction_id);
    fs::create_dir_all(&transaction_dir).map_err(|e| e.to_string())?;
    fs::write(transaction_dir.join("original-index"), &original_bytes)
        .map_err(|e| e.to_string())?;
    fs::write(transaction_dir.join("result-index"), &retained).map_err(|e| e.to_string())?;
    let mut journal = Journal {
        old_head: initial.head.clone(),
        expected_tree: candidate_tree.clone(),
        original_index_hash: hash(&original_bytes),
        result_index_hash: hash(&retained),
        new_head: None,
        pid: std::process::id(),
        post_hook_index_hash: None,
        post_hook_conflict: None,
    };
    save_journal(&transaction_dir, &journal)?;
    lock.write(&retained)?;
    let mut args: Vec<OsString> = vec![
        "-c".into(),
        format!("core.hooksPath={}", hooks.display()).into(),
        "commit".into(),
        "-F".into(),
        message.into_os_string(),
    ];
    if request.amend {
        args.push("--amend".into());
    }
    let outcome = committing.run(args);
    let mut verifying = git.clone();
    verifying.context.cancel = Arc::new(AtomicBool::new(false));
    let new_head = repository::head(&verifying)?;
    let changed = new_head != initial.head;
    let committed_tree = new_head
        .as_ref()
        .map(|h| verifying.text(["rev-parse", &format!("{h}^{{tree}}")]))
        .transpose()?;
    if changed && committed_tree.as_ref() == Some(&candidate_tree) {
        journal.new_head = new_head.clone();
        save_journal(&transaction_dir, &journal)?;
        let mut post_hook_git = candidate.clone();
        post_hook_git.context.cancel = Arc::new(AtomicBool::new(false));
        let post_hook_tree = post_hook_git.text(["write-tree"]);
        if post_hook_tree.as_ref().ok() != Some(&candidate_tree) {
            post_hook_git.run(["update-index", "--no-split-index"])?;
            let post_hook_bytes =
                fs::read(temp.path().join("candidate-index")).map_err(|e| e.to_string())?;
            fs::write(transaction_dir.join("post-hook-index"), &post_hook_bytes)
                .map_err(|e| e.to_string())?;
            journal.post_hook_index_hash = Some(hash(&post_hook_bytes));
            let reconciled = (|| {
                let post_tree = post_hook_tree?;
                let changed = verifying.run([
                    "diff",
                    "--name-only",
                    "-z",
                    &candidate_tree,
                    &post_tree,
                    "--",
                ])?;
                let post_paths = changed
                    .split(|b| *b == 0)
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_vec())
                    .collect();
                let post_temp = tempfile::Builder::new()
                    .prefix("gitgui-commit-post-")
                    .tempdir_in(&git_dir)
                    .map_err(|e| e.to_string())?;
                let retained_git = temporary_index(
                    &verifying,
                    post_temp.path(),
                    "retained-index",
                    Some(&retained),
                    None,
                )?;
                retained_index(
                    &verifying,
                    &retained_git,
                    &post_hook_git,
                    &candidate_tree,
                    post_temp.path(),
                    &retained,
                    &post_paths,
                )
            })();
            match reconciled {
                Ok(bytes) => {
                    fs::write(transaction_dir.join("result-index"), &bytes)
                        .map_err(|e| e.to_string())?;
                    journal.result_index_hash = hash(&bytes);
                    journal.post_hook_index_hash = None;
                    save_journal(&transaction_dir, &journal)?;
                    lock.write(&bytes)?;
                }
                Err(error) => {
                    journal.post_hook_conflict = Some(error);
                    save_journal(&transaction_dir, &journal)?;
                    lock.finish()?;
                    return Err(format!("提交 {} 已完成。提交后钩子的暂存改动与原暂存内容冲突；两份索引均已保存，请在恢复记录中选择保留方式。", &new_head.as_ref().ok_or("提交编号缺失")?[..8]));
                }
            }
        }
        lock.finish()?;
        fs::remove_dir_all(&transaction_dir)
            .map_err(|e| format!("提交已完成，但恢复记录清理失败：{e}"))?;
        let oid = new_head.ok_or("提交后 HEAD 丢失")?;
        if request.push {
            git.run(["push"])
                .map_err(|e| format!("提交 {} 已完成，推送失败：{e}", &oid[..8]))?;
            Ok(format!("已提交并推送 {}", &oid[..8]))
        } else {
            Ok(format!("已提交 {}", &oid[..8]))
        }
    } else if !changed {
        fs::remove_dir_all(&transaction_dir).map_err(|e| e.to_string())?;
        match outcome {
            Err(error) => Err(error),
            Ok(_) => Err("Git 返回成功，但未找到预期提交；原暂存内容已保留".into()),
        }
    } else {
        Err(format!("HEAD 已变化但与预期提交不一致。已保留原索引及恢复记录 {transaction_id}，请检查仓库状态后处理恢复"))
    }
}

fn save_journal(directory: &Path, journal: &Journal) -> Result<()> {
    let path = directory.join("transaction.json");
    let bytes = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn recover(git: &Git, id: &str, choice: &str) -> Result<String> {
    if !["original", "hook"].contains(&choice) {
        return Err("恢复选择无效".into());
    }
    uuid::Uuid::parse_str(id).map_err(|_| "恢复记录编号无效")?;
    let directory = git.path("gitgui-recovery")?.join(id);
    let journal: Journal = serde_json::from_slice(
        &fs::read(directory.join("transaction.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let current_head = repository::head(git)?;
    let index = repository::index_bytes(git)?;
    if journal.post_hook_index_hash.is_some() {
        if hash(&index) != journal.result_index_hash && hash(&index) != journal.original_index_hash
        {
            return Err("暂存区已被后续操作修改，不能覆盖".into());
        }
        if current_head != journal.new_head {
            return Err("当前分支已移动，不能恢复旧提交的暂存状态".into());
        }
        let (filename, expected_hash) = if choice == "hook" {
            (
                "post-hook-index",
                journal
                    .post_hook_index_hash
                    .as_ref()
                    .ok_or("缺少钩子索引校验值")?,
            )
        } else {
            ("result-index", &journal.result_index_hash)
        };
        let bytes = fs::read(directory.join(filename)).map_err(|e| e.to_string())?;
        if hash(&bytes) != *expected_hash {
            return Err("索引备份校验失败".into());
        }
        let lock = IndexLock::acquire(git)?;
        lock.write(&bytes)?;
        lock.finish()?;
        fs::remove_dir_all(directory).map_err(|e| e.to_string())?;
        return Ok(if choice == "hook" {
            "已采用后置钩子的暂存状态"
        } else {
            "已保留原来的剩余暂存内容"
        }
        .into());
    }
    if choice == "hook" {
        return Err("该恢复记录不包含后置钩子索引，请选择保留原暂存内容".into());
    }
    if hash(&index) == journal.result_index_hash {
        fs::remove_dir_all(directory).map_err(|e| e.to_string())?;
        return Ok("暂存区已经恢复，已清理恢复记录".into());
    }
    if hash(&index) != journal.original_index_hash {
        return Err("暂存区在中断后已被修改，不能自动覆盖；原始和候选索引仍保留在恢复目录".into());
    }
    let lock_path = git.path("index")?.with_extension("lock");
    if lock_path.exists() {
        #[cfg(unix)]
        {
            let running = unsafe { libc::kill(journal.pid as i32, 0) } == 0;
            if running {
                return Err("创建恢复记录的进程仍在运行，请等待它退出".into());
            }
        }
        if hash(&fs::read(&lock_path).map_err(|e| e.to_string())?) != journal.result_index_hash {
            return Err("锁文件并非本次恢复记录创建，无法移除".into());
        }
        fs::remove_file(lock_path).map_err(|e| e.to_string())?;
    }
    if current_head == journal.old_head {
        fs::remove_dir_all(directory).map_err(|e| e.to_string())?;
        return Ok("提交未执行，原工作区和暂存区已保留".into());
    }
    let actual = current_head.as_ref().ok_or("当前 HEAD 不存在")?;
    if journal.new_head.as_ref().is_some_and(|h| h != actual)
        || git.text(["rev-parse", &format!("{actual}^{{tree}}")])? != journal.expected_tree
    {
        return Err("当前提交与恢复记录不一致，已停止自动恢复".into());
    }
    let retained = fs::read(directory.join("result-index")).map_err(|e| e.to_string())?;
    if hash(&retained) != journal.result_index_hash {
        return Err("恢复索引校验失败".into());
    }
    let lock = IndexLock::acquire(git)?;
    lock.write(&retained)?;
    lock.finish()?;
    fs::remove_dir_all(directory).map_err(|e| e.to_string())?;
    Ok("提交后的暂存区已恢复".into())
}
