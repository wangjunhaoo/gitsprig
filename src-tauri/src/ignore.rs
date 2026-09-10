use crate::{
    commit::IndexLock,
    git::{os_bytes, Git, Result},
    model::RepositoryStatus,
    repository,
};
use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::Path,
};

const MAX_IGNORE: u64 = 1024 * 1024;

fn read_ignore(path: &Path) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法检查 .gitignore：{error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(".gitignore 必须是普通文件，不能是目录或符号链接".into());
    }
    if metadata.len() > MAX_IGNORE || metadata.permissions().readonly() {
        return Err(".gitignore 超过 1 MB 或处于只读状态，未修改文件".into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("无法读取 .gitignore：{e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(".gitignore 文件类型已变化".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_IGNORE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_IGNORE {
        return Err(".gitignore 超过 1 MB".into());
    }
    Ok(Some(bytes))
}

fn pattern(path: &Path, by_name: bool) -> Result<Vec<u8>> {
    let bytes = if by_name {
        os_bytes(path.file_name().ok_or("无法识别文件名")?)
    } else {
        os_bytes(path.as_os_str())
    };
    if bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r' | 0)) {
        return Err("所选名称含有换行符，无法生成精确的忽略规则，未修改 .gitignore".into());
    }
    let mut result = if by_name { vec![] } else { vec![b'/'] };
    for byte in bytes {
        if matches!(byte, b'\\' | b'*' | b'?' | b'[' | b']' | b'!' | b'#' | b' ') {
            result.push(b'\\');
        }
        result.push(byte);
    }
    Ok(result)
}

pub fn apply(git: &Git, state: &RepositoryStatus, ids: &[String], by_name: bool) -> Result<String> {
    if ids.is_empty() {
        return Err("请先选择需要忽略的未跟踪文件".into());
    }
    let mut paths = Vec::new();
    let mut intent_paths = Vec::new();
    let mut patterns = Vec::new();
    let mut seen = HashSet::new();
    for id in ids {
        let file = state
            .files
            .iter()
            .find(|file| &file.id == id)
            .ok_or("所选文件已不在改动列表中，请重新选择")?;
        if !(file.untracked || file.intent_to_add) || file.conflict || file.submodule {
            return Err(
                "忽略仅适用于未跟踪或待加入 Git 的文件，已跟踪文件及其暂存内容保持不变".into(),
            );
        }
        repository::safe_work_path(git, id)?;
        let path = repository::decode_path(id)?;
        if file.intent_to_add {
            intent_paths.push(path.clone().into_os_string());
        }
        let rule = pattern(&path, by_name)?;
        if seen.insert(rule.clone()) {
            patterns.push(rule);
        }
        paths.push(path.into_os_string());
    }
    let index_lock = IndexLock::acquire(git)?;
    if repository::status(git)?.snapshot != state.snapshot {
        return Err("仓库状态已变化，请重新选择需要忽略的文件".into());
    }
    let original_index = repository::index_bytes(git)?;
    let index_directory =
        tempfile::tempdir_in(git.path("index")?.parent().ok_or("暂存区目录缺失")?)
            .map_err(|e| e.to_string())?;
    let candidate_index = index_directory.path().join("index");
    let index_git = if intent_paths.is_empty() {
        git.clone()
    } else {
        fs::write(&candidate_index, &original_index).map_err(|e| e.to_string())?;
        let candidate_git = git.index(&candidate_index);
        let mut remove = vec![
            OsString::from("update-index"),
            "--force-remove".into(),
            "--".into(),
        ];
        remove.extend(intent_paths.clone());
        candidate_git.run(remove)?;
        candidate_git.run(["update-index", "--no-split-index"])?;
        index_lock.write(&fs::read(&candidate_index).map_err(|e| e.to_string())?)?;
        candidate_git
    };
    let target = git.root.join(".gitignore");
    let previous = read_ignore(&target)?;
    let original = previous.as_deref().unwrap_or_default();
    let newline: &[u8] = if original.windows(2).any(|bytes| bytes == b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    };
    let mut candidate = original.to_vec();
    if previous.is_some() {
        candidate.extend_from_slice(newline);
    }
    for rule in &patterns {
        candidate.extend_from_slice(rule);
        candidate.extend_from_slice(newline);
    }
    if candidate.len() as u64 > MAX_IGNORE {
        return Err("追加后 .gitignore 将超过 1 MB，未修改文件".into());
    }
    let mut tracked = vec![
        OsString::from("ls-files"),
        "--cached".into(),
        "-z".into(),
        "--".into(),
    ];
    tracked.extend(paths.clone());
    if !index_git.run(tracked)?.is_empty() {
        return Err("所选文件已被 Git 跟踪，请重新选择".into());
    }
    let current = read_ignore(&target)?;
    if current != previous {
        return Err(".gitignore 在操作期间被修改，请刷新后重试".into());
    }
    if repository::index_bytes(git)? != original_index {
        return Err("暂存区在操作期间被修改，未写入忽略规则".into());
    }
    if previous.is_some() {
        // 追加模式不会覆盖检查后由外部编辑器写入的内容；首个换行隔开未结束的行。
        let mut options = fs::OpenOptions::new();
        options.append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options
            .open(&target)
            .map_err(|e| format!("无法追加 .gitignore：{e}"))?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err(".gitignore 文件类型已变化，未追加规则".into());
        }
        file.write_all(&candidate[original.len()..])
            .map_err(|e| format!("追加忽略规则失败，请核对 .gitignore：{e}"))?;
        file.sync_all()
            .map_err(|e| format!("规则已追加，但同步文件失败：{e}"))?;
    } else {
        let mut temporary =
            tempfile::NamedTempFile::new_in(&git.root).map_err(|e| e.to_string())?;
        temporary.write_all(&candidate).map_err(|e| e.to_string())?;
        temporary.as_file().sync_all().map_err(|e| e.to_string())?;
        temporary
            .persist_noclobber(&target)
            .map_err(|e| format!("无法创建 .gitignore，原有文件保持不变：{e}"))?;
    }
    if read_ignore(&target)?.as_deref() != Some(candidate.as_slice()) {
        return Err(
            "忽略规则已追加，但 .gitignore 同时被外部修改，请刷新核对；待加入标记未清除".into(),
        );
    }
    if !intent_paths.is_empty() {
        if repository::index_bytes(git)? != original_index {
            return Err("忽略规则已写入，但暂存区已变化，待加入标记未清除".into());
        }
        index_lock
            .finish()
            .map_err(|e| format!("忽略规则已写入，但清除待加入标记失败：{e}"))?;
    }
    #[cfg(unix)]
    fs::File::open(&git.root)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| format!("忽略规则已写入，但目录同步失败：{e}"))?;
    let mut visible = vec![
        OsString::from("ls-files"),
        "--others".into(),
        "--exclude-standard".into(),
        "-z".into(),
        "--".into(),
    ];
    visible.extend(paths);
    let remaining = git
        .run(visible)
        .map_err(|e| format!("忽略规则已写入，但核对结果失败：{e}"))?;
    let remaining_count = remaining
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .count();
    if remaining_count > 0 {
        return Ok(format!("已向 .gitignore 添加 {} 条规则，原文件保留；仍有 {remaining_count} 项未被忽略，请检查子目录中的反向忽略规则。", patterns.len()));
    }
    Ok(format!(
        "已向 .gitignore 添加 {} 条忽略规则，所选文件保留在本地。",
        patterns.len()
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::pattern;
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

    #[test]
    fn ignore_pattern_preserves_raw_filename_bytes() {
        let path = Path::new(OsStr::from_bytes(b"dir/raw-\xff*.txt"));
        assert_eq!(pattern(path, false).unwrap(), b"/dir/raw-\xff\\*.txt");
        assert_eq!(pattern(path, true).unwrap(), b"raw-\xff\\*.txt");
    }
}
