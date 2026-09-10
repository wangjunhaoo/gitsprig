use crate::{
    commit,
    git::{self, Git, Result},
    model::*,
    repository::{self, decode_path, resolve_commit},
};
use serde_json::{json, Value};
use std::{ffi::OsString, fs, path::Path};

fn required(args: &[String], index: usize) -> Result<&str> {
    args.get(index)
        .map(String::as_str)
        .filter(|s| !s.is_empty() && !s.contains('\0'))
        .ok_or_else(|| "操作参数不完整".into())
}
fn name(value: &str) -> Result<&str> {
    if value.is_empty() || value.starts_with('-') || value.contains('\0') {
        Err("名称无效，不能以短横线开头".into())
    } else {
        Ok(value)
    }
}
fn confirm(action: &GitAction) -> Result<()> {
    if !action.confirmed {
        Err("请先确认该操作的影响范围".into())
    } else {
        Ok(())
    }
}
fn branch(git: &Git, value: &str) -> Result<String> {
    let value = name(value)?;
    git.text(["check-ref-format", "--branch", value])
}

pub fn execute(git: &Git, action: &GitAction) -> Result<String> {
    let state = repository::status(git)?;
    if let Some(snapshot) = &action.snapshot {
        if *snapshot != state.snapshot {
            return Err("仓库状态已变化，请刷新后重新确认操作".into());
        }
    }
    if !state.recovery.is_empty() && action.kind != "recoverIndex" {
        return Err("请先处理提交恢复记录".into());
    }
    if state.operation.is_some()
        && ![
            "continue",
            "skip",
            "abort",
            "recoverIndex",
            "amendPaused",
            "editTodo",
            "stage",
            "unstage",
        ]
        .contains(&action.kind.as_str())
    {
        return Err("请先完成或终止当前 Git 操作".into());
    }
    let args = &action.args;
    let mut command: Vec<OsString> = vec![];
    let s = |value: &str| OsString::from(value);
    match action.kind.as_str() {
        "ignorePaths" | "ignoreNames" => {
            return crate::ignore::apply(git, &state, args, action.kind == "ignoreNames");
        }
        "fetch" => {
            command = vec![s("fetch"), s("--all"), s("--progress")];
        }
        "pull" => {
            command = vec![s("pull"), s("--ff-only"), s("--progress")];
        }
        "pullMerge" => {
            command = vec![s("pull"), s("--no-rebase"), s("--no-edit"), s("--progress")];
        }
        "pullRebase" => {
            command = vec![
                s("-c"),
                s("rebase.autoStash=false"),
                s("pull"),
                s("--rebase=merges"),
                s("--progress"),
            ];
        }
        "switch" => {
            command = vec![s("switch"), s("--"), s(name(required(args, 0)?)?)];
        }
        "track" => {
            command = vec![
                s("switch"),
                s("--track"),
                s("--"),
                s(name(required(args, 0)?)?),
            ];
        }
        "branchCreate" => {
            let target = branch(git, required(args, 0)?)?;
            command = vec![s("switch"), s("-c"), s(&target)];
            if args.len() > 1 && !args[1].is_empty() {
                command.push(s(&resolve_commit(git, &args[1])?));
            }
        }
        "branchRename" => {
            command = vec![
                s("branch"),
                s("-m"),
                s(&branch(git, required(args, 0)?)?),
                s(&branch(git, required(args, 1)?)?),
            ];
        }
        "branchDelete" | "branchDeleteForce" => {
            confirm(action)?;
            command = vec![
                s("branch"),
                s(if action.kind == "branchDeleteForce" {
                    "-D"
                } else {
                    "-d"
                }),
                s("--"),
                s(&branch(git, required(args, 0)?)?),
            ];
        }
        "upstream" => {
            command = vec![
                s("branch"),
                s(&format!("--set-upstream-to={}", name(required(args, 0)?)?)),
            ];
        }
        "merge" => {
            command = vec![
                s("merge"),
                s("--no-edit"),
                s(&resolve_commit(git, required(args, 0)?)?),
            ];
        }
        "rebase" | "interactiveRebase" => {
            let dirty = !state.files.is_empty();
            if dirty {
                return Err("变基前请先提交或 Stash 工作区及暂存区改动".into());
            }
            confirm(action)?;
            command = vec![
                s("-c"),
                s("rebase.autoStash=false"),
                s("rebase"),
                s("--rebase-merges"),
            ];
            if action.kind == "interactiveRebase" {
                command.push(s("-i"));
            }
            if required(args, 0)? == "--root" {
                command.push(s("--root"));
            } else {
                command.push(s(&resolve_commit(git, required(args, 0)?)?));
            }
        }
        "cherryPick" | "revert" => {
            if action.kind == "revert" {
                confirm(action)?;
            }
            command = vec![s(if action.kind == "cherryPick" {
                "cherry-pick"
            } else {
                "revert"
            })];
            if action.kind == "revert" {
                command.push(s("--no-edit"));
            }
            let oid = resolve_commit(git, required(args, 0)?)?;
            let parents = git.text(["rev-list", "--parents", "-n", "1", &oid])?;
            if parents.split_whitespace().count() > 2 {
                let parent = required(args, 1)?
                    .parse::<usize>()
                    .map_err(|_| "合并提交需要选择主线父提交编号")?;
                if parent == 0 || parent >= parents.split_whitespace().count() {
                    return Err("主线父提交编号超出范围".into());
                }
                command.extend([s("-m"), s(&parent.to_string())]);
            }
            command.push(s(&oid));
        }
        "reset" => {
            confirm(action)?;
            let mode = required(args, 0)?;
            if !["soft", "mixed", "hard"].contains(&mode) {
                return Err("无效的重置模式".into());
            }
            command = vec![
                s("reset"),
                s(&format!("--{mode}")),
                s(&resolve_commit(git, required(args, 1)?)?),
            ];
        }
        "tagCreate" => {
            let tag = name(required(args, 0)?)?;
            git.run(["check-ref-format", &format!("refs/tags/{tag}")])?;
            let oid = resolve_commit(git, required(args, 1)?)?;
            command = vec![s("tag")];
            if let Some(message) = args.get(2).filter(|m| !m.is_empty()) {
                command.extend([s("-a"), s("-m"), s(message)]);
            }
            command.extend([s("--"), s(tag), s(&oid)]);
        }
        "tagDelete" => {
            confirm(action)?;
            command = vec![s("tag"), s("-d"), s("--"), s(name(required(args, 0)?)?)];
        }
        "tagPush" => {
            command = vec![
                s("push"),
                s(name(required(args, 0)?)?),
                s(&format!("refs/tags/{}", name(required(args, 1)?)?)),
            ];
        }
        "remoteAdd" => {
            command = vec![
                s("remote"),
                s("add"),
                s("--"),
                s(name(required(args, 0)?)?),
                s(name(required(args, 1)?)?),
            ];
        }
        "remoteSetUrl" => {
            command = vec![
                s("remote"),
                s("set-url"),
                s("--"),
                s(name(required(args, 0)?)?),
                s(name(required(args, 1)?)?),
            ];
        }
        "remoteRemove" => {
            confirm(action)?;
            command = vec![
                s("remote"),
                s("remove"),
                s("--"),
                s(name(required(args, 0)?)?),
            ];
        }
        "stashCreate" => {
            command = vec![s("stash"), s("push"), s("-m"), s(required(args, 0)?)];
            if args.get(1).is_some_and(|v| v == "true") {
                command.push(s("--include-untracked"));
            }
            if args.get(2).is_some_and(|v| v == "true") {
                command.push(s("--keep-index"));
            }
        }
        "stashApply" | "stashPop" | "stashDrop" => {
            let stash = required(args, 0)?;
            let oid = resolve_commit(git, stash)?;
            if (action.kind == "stashDrop" || action.kind == "stashPop")
                && (!stash.starts_with("stash@{")
                    || !stash.ends_with('}')
                    || stash[7..stash.len() - 1].parse::<u32>().is_err())
            {
                return Err("请选择当前 Stash 列表中的条目".into());
            }
            if action.kind == "stashDrop" {
                confirm(action)?;
            }
            command = vec![
                s("stash"),
                s(match action.kind.as_str() {
                    "stashApply" => "apply",
                    "stashPop" => "pop",
                    _ => "drop",
                }),
            ];
            if action.kind != "stashDrop" && args.get(1).is_some_and(|v| v == "true") {
                command.push(s("--index"));
            }
            command.push(s(if action.kind == "stashApply" {
                &oid
            } else {
                stash
            }));
        }
        "worktreeAdd" => {
            let path = required(args, 0)?;
            if !Path::new(path).is_absolute() {
                return Err("工作树目录必须是绝对路径".into());
            }
            let branch_name = branch(git, required(args, 1)?)?;
            command = vec![s("worktree"), s("add")];
            if args.get(2).is_some_and(|v| v == "new") {
                command.extend([s("-b"), s(&branch_name), s("--"), s(path)]);
                if let Some(base) = args.get(3).filter(|v| !v.is_empty()) {
                    command.push(s(&resolve_commit(git, base)?));
                }
            } else {
                command.extend([s("--"), s(path), s(&branch_name)]);
            }
        }
        "worktreeRemove" => {
            confirm(action)?;
            command = vec![s("worktree"), s("remove"), s("--"), s(required(args, 0)?)];
        }
        "reflogRestore" => {
            let target = branch(git, required(args, 1)?)?;
            command = vec![
                s("branch"),
                s(&target),
                s(&resolve_commit(git, required(args, 0)?)?),
            ];
        }
        "continue" | "skip" | "abort" => {
            let operation = state.operation.ok_or("当前没有可继续的 Git 操作")?;
            if action.kind == "continue" && !operation.can_continue {
                return Err("请先标记解决所有冲突".into());
            }
            if action.kind == "skip" && !operation.can_skip {
                return Err("当前操作不支持跳过".into());
            }
            if action.kind != "continue" {
                confirm(action)?;
            }
            command = vec![s(&operation.kind), s(&format!("--{}", action.kind))];
        }
        "amendPaused" => {
            if state.operation.is_none_or(|o| o.kind != "rebase") {
                return Err("仅在变基暂停时允许此操作".into());
            }
            command = vec![s("commit"), s("--amend"), s("-m"), s(required(args, 0)?)];
        }
        "editTodo" => {
            if state.operation.is_none_or(|o| o.kind != "rebase") {
                return Err("仅在变基期间可以编辑剩余提交序列".into());
            }
            command = vec![s("rebase"), s("--edit-todo")];
        }
        "discard" => {
            confirm(action)?;
            let id = required(args, 0)?;
            let file = state
                .files
                .iter()
                .find(|f| f.id == id)
                .ok_or("文件不在当前改动列表中")?;
            if file.submodule {
                return Err("请在子模块仓库内处理其改动".into());
            }
            let path = repository::safe_work_path(git, id)?;
            if file.untracked {
                fs::remove_file(path).map_err(|e| e.to_string())?;
                return Ok("已删除所选未跟踪文件".into());
            }
            if state.head.is_none() {
                return Err("空仓库的新增文件请先取消暂存，再删除不需要的文件".into());
            }
            command = vec![
                s("restore"),
                s("--source=HEAD"),
                s("--staged"),
                s("--worktree"),
                s("--"),
                decode_path(id)?.into_os_string(),
            ];
            if let Some(ref old) = file.old_id {
                command.push(decode_path(old)?.into_os_string());
            }
        }
        "unstage" => {
            let path = decode_path(required(args, 0)?)?;
            command = if state.head.is_some() {
                vec![s("restore"), s("--staged"), s("--"), path.into_os_string()]
            } else {
                vec![s("rm"), s("--cached"), s("--"), path.into_os_string()]
            };
        }
        "stage" => {
            command = vec![
                s("add"),
                s("-A"),
                s("--"),
                decode_path(required(args, 0)?)?.into_os_string(),
            ];
        }
        "recoverIndex" => {
            confirm(action)?;
            return commit::recover(git, required(args, 0)?, required(args, 1)?);
        }
        _ => return Err("未知或未授权的 Git 操作".into()),
    }
    let output = git.run(command)?;
    let text = String::from_utf8_lossy(&output);
    if text.trim().is_empty() {
        Ok("操作已完成".into())
    } else {
        Ok(git::redact(text.trim()))
    }
}

pub fn collections(git: &Git, kind: &str) -> Result<Value> {
    match kind {
        "stash" => {
            let data = git.run(["stash", "list", "--format=%gd%x00%H%x00%gs%x00%at%x00"])?;
            let fields: Vec<_> = data.split(|b| *b == 0).collect();
            Ok(Value::Array(fields.chunks(4).filter(|r| r.len() == 4).map(|r| json!({"name":String::from_utf8_lossy(r[0]).trim(),"oid":String::from_utf8_lossy(r[1]),"subject":String::from_utf8_lossy(r[2]),"timestamp":String::from_utf8_lossy(r[3])})).collect()))
        }
        "reflog" => {
            if repository::head(git)?.is_none() {
                return Ok(json!([]));
            }
            let data = git.run([
                "reflog",
                "-n",
                "300",
                "--format=%gd%x00%H%x00%gs%x00%at%x00",
            ])?;
            let fields: Vec<_> = data.split(|b| *b == 0).collect();
            Ok(Value::Array(fields.chunks(4).filter(|r| r.len() == 4).map(|r| json!({"name":String::from_utf8_lossy(r[0]).trim(),"oid":String::from_utf8_lossy(r[1]),"subject":String::from_utf8_lossy(r[2]),"timestamp":String::from_utf8_lossy(r[3])})).collect()))
        }
        "worktree" => {
            let data = git.run(["worktree", "list", "--porcelain", "-z"])?;
            let mut result = vec![];
            let mut item = serde_json::Map::new();
            for record in data.split(|b| *b == 0) {
                if record.is_empty() {
                    if !item.is_empty() {
                        result.push(Value::Object(std::mem::take(&mut item)));
                    }
                    continue;
                }
                let split = record
                    .iter()
                    .position(|b| *b == b' ')
                    .unwrap_or(record.len());
                let key = String::from_utf8_lossy(&record[..split]).into_owned();
                let value = if split < record.len() {
                    String::from_utf8_lossy(&record[split + 1..]).into_owned()
                } else {
                    "true".into()
                };
                item.insert(key, Value::String(value));
            }
            if !item.is_empty() {
                result.push(Value::Object(item));
            }
            Ok(Value::Array(result))
        }
        _ => Err("未知的仓库集合类型".into()),
    }
}
