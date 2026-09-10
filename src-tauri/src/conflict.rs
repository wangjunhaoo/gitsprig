use crate::{
    git::{hash, Git, Result},
    model::ConflictFile,
    repository,
};
use std::{ffi::OsString, fs, io::Write};

fn stage(git: &Git, path: &std::path::Path, number: u8) -> Result<Option<Vec<u8>>> {
    let mut spec = OsString::from(format!(":{number}:"));
    spec.push(path);
    let output = git.raw(vec!["cat-file".into(), "-s".into(), spec.clone()])?;
    if output.code != 0 {
        return Ok(None);
    }
    let size: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|_| "冲突对象大小无效")?;
    if size > repository::FULL_LIMIT {
        return Err("冲突文件超过 32 MB，请使用外部工具解决后在此标记完成".into());
    }
    Ok(Some(git.run(vec![
        "cat-file".into(),
        "blob".into(),
        spec,
    ])?))
}

pub fn read(git: &Git, id: &str) -> Result<ConflictFile> {
    let path = repository::decode_path(id)?;
    let state = repository::status(git)?;
    if !state.files.iter().any(|f| f.id == id && f.conflict) {
        return Err("该文件已不处于冲突状态".into());
    }
    let base = stage(git, &path, 1)?;
    let ours = stage(git, &path, 2)?;
    let theirs = stage(git, &path, 3)?;
    let result = repository::work_content(git, id, repository::FULL_LIMIT)?;
    let binary = [&base, &ours, &theirs].iter().any(|v| {
        v.as_ref()
            .is_some_and(|b| b.contains(&0) || std::str::from_utf8(b).is_err())
    }) || result.contains(&0)
        || std::str::from_utf8(&result).is_err();
    let rebase = state.operation.is_some_and(|o| o.kind == "rebase");
    let decode = |value: Option<Vec<u8>>| {
        if binary {
            None
        } else {
            value.and_then(|v| String::from_utf8(v).ok())
        }
    };
    let ours_exists = ours.is_some();
    let theirs_exists = theirs.is_some();
    Ok(ConflictFile {
        file_id: id.into(),
        path: path.to_string_lossy().into_owned(),
        snapshot: state.snapshot,
        base: decode(base),
        ours: decode(ours),
        theirs: decode(theirs),
        result: if binary {
            None
        } else {
            Some(String::from_utf8(result.clone()).map_err(|e| e.to_string())?)
        },
        ours_label: if rebase {
            "变基目标与已重放提交（ours）"
        } else {
            "当前分支（ours）"
        }
        .into(),
        theirs_label: if rebase {
            "正在重放的提交（theirs）"
        } else {
            "合入版本（theirs）"
        }
        .into(),
        binary,
        ours_exists,
        theirs_exists,
        result_hash: hash(&result),
    })
}

pub fn save(git: &Git, id: &str, expected_hash: &str, content: &str) -> Result<String> {
    let current = read(git, id)?;
    if current.result_hash != expected_hash {
        return Err("冲突文件已被外部修改，请重新加载后继续编辑".into());
    }
    if current.binary {
        return Err("二进制文件不能写入文本合并结果".into());
    }
    if content.len() as u64 > repository::FULL_LIMIT {
        return Err("合并结果超过文件大小限制".into());
    }
    let path = repository::safe_work_path(git, id)?;
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("符号链接冲突请选择整个来源版本".into());
    }
    let parent = path.parent().ok_or("文件没有父目录")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .map_err(|e| e.to_string())?;
    }
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(&path).map_err(|e| e.to_string())?;
    Ok(hash(content.as_bytes()))
}

pub fn choose(git: &Git, id: &str, side: &str, expected_hash: &str) -> Result<String> {
    let current = read(git, id)?;
    if current.result_hash != expected_hash {
        return Err("文件已经变化，请重新加载".into());
    }
    let exists = match side {
        "ours" => current.ours_exists,
        "theirs" => current.theirs_exists,
        "delete" => false,
        _ => return Err("冲突来源无效".into()),
    };
    let path = repository::safe_work_path(git, id)?;
    if !exists {
        if fs::symlink_metadata(&path).is_ok() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    } else {
        git.run(vec![
            "checkout".into(),
            format!("--{side}").into(),
            "--".into(),
            repository::decode_path(id)?.into_os_string(),
        ])?;
    }
    Ok("已保存所选文件版本；检查后点击“标记解决”".into())
}

pub fn mark(git: &Git, id: &str, expected_hash: &str) -> Result<String> {
    let current = read(git, id)?;
    if current.result_hash != expected_hash {
        return Err("文件已发生变化，请先检查最新内容".into());
    }
    if let Some(content) = &current.result {
        if has_markers(content) {
            return Err("文件仍包含冲突标记，请完成所有冲突区块后再标记解决".into());
        }
    }
    git.run(vec![
        "add".into(),
        "-A".into(),
        "--".into(),
        repository::decode_path(id)?.into_os_string(),
    ])?;
    Ok("已标记解决".into())
}

pub fn has_markers(content: &str) -> bool {
    content.lines().any(|line| {
        let count = line
            .bytes()
            .take_while(|b| *b == b'<' || *b == b'>')
            .count();
        count >= 7 && (line.starts_with("<<<<<<<") || line.starts_with(">>>>>>>"))
    })
}
