use crate::{
    git::{self, Git, Result},
    repository,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ffi::OsString};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranch {
    pub name: String,
    pub oid: Option<String>,
    pub destinations: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub remote: String,
    pub urls: Vec<String>,
    pub configuration: String,
    pub branches: Vec<RemoteBranch>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PushRequest {
    pub remote: String,
    pub configuration: String,
    pub source: String,
    pub source_oid: String,
    pub target: String,
    pub force: bool,
    pub expected: Option<String>,
    pub set_upstream: bool,
}

fn push_urls(git: &Git, remote: &str) -> Result<Vec<String>> {
    if remote.starts_with('-') || !git.text(["remote"])?.lines().any(|name| name == remote) {
        return Err("远程仓库配置已变化，请重新打开推送窗口".into());
    }
    for key in [
        format!("remote.{remote}.url"),
        format!("remote.{remote}.pushurl"),
    ] {
        let values = git.raw(["config", "--null", "--get-all", &key])?;
        if values.code != 0 && values.code != 1 {
            return Err("无法读取远程地址配置".into());
        }
        if values.stdout.contains(&b'\n') || values.stdout.contains(&b'\r') {
            return Err("推送地址包含换行符，无法列出远程分支".into());
        }
    }
    let urls: Vec<_> = git
        .text(["remote", "get-url", "--push", "--all", "--", remote])?
        .lines()
        .map(String::from)
        .collect();
    if urls.is_empty()
        || urls
            .iter()
            .any(|url| url.is_empty() || url.starts_with('-'))
    {
        return Err("远程仓库没有有效的推送地址".into());
    }
    Ok(urls)
}

fn configuration(remote: &str, urls: &[String]) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{remote}\0{}", urls.join("\0")).as_bytes())
    )
}

fn valid_oid(oid: &str) -> bool {
    [40, 64].contains(&oid.len()) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn remote_info(git: &Git, remote: &str) -> Result<RemoteInfo> {
    let urls = push_urls(git, remote)?;
    let mut branches: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for url in &urls {
        let output = git.text(["ls-remote", "--heads", "--refs", "--quiet", "--", url])?;
        let mut seen = std::collections::HashSet::new();
        for line in output.lines() {
            let (oid, reference) = line.split_once('\t').ok_or("远端分支响应格式无效")?;
            let name = reference
                .strip_prefix("refs/heads/")
                .ok_or("远端返回了非分支引用")?;
            if !valid_oid(oid) || !seen.insert(name) {
                return Err("远端分支响应无效或重复".into());
            }
            branches.entry(name.into()).or_default().push(oid.into());
        }
    }
    let current = push_urls(git, remote)?;
    if current != urls {
        return Err("读取期间推送地址已变化，请重新读取分支".into());
    }
    Ok(RemoteInfo {
        remote: remote.into(),
        configuration: configuration(remote, &urls),
        urls: urls.iter().map(|url| git::redact(url)).collect(),
        branches: branches
            .into_iter()
            .map(|(name, oids)| RemoteBranch {
                name,
                oid: if oids.len() == urls.len() && oids.iter().all(|oid| oid == &oids[0]) {
                    Some(oids[0].clone())
                } else {
                    None
                },
                destinations: oids.len(),
            })
            .collect(),
    })
}

pub fn execute(git: &Git, request: &PushRequest) -> Result<String> {
    let state = repository::status(git)?;
    if state.operation.is_some() || !state.recovery.is_empty() {
        return Err("请先完成当前 Git 操作或提交恢复，再推送".into());
    }
    let local = if request.source == "HEAD" {
        if request.set_upstream {
            return Err("游离 HEAD 不能设置分支上游".into());
        }
        None
    } else {
        let local = request
            .source
            .strip_prefix("refs/heads/")
            .ok_or("请选择本地分支作为推送来源")?;
        git.run(["check-ref-format", &request.source])?;
        Some(local)
    };
    if !valid_oid(&request.source_oid)
        || repository::resolve_commit(git, &request.source)? != request.source_oid
    {
        return Err("本地分支提交已变化，请重新打开推送窗口确认".into());
    }
    if request.target.starts_with('-') {
        return Err("目标分支不能以短横线开头".into());
    }
    let target = format!("refs/heads/{}", request.target);
    git.run(["check-ref-format", &target])?;
    let urls = push_urls(git, &request.remote)?;
    if configuration(&request.remote, &urls) != request.configuration {
        return Err("推送地址已变化，请重新读取分支并确认目标".into());
    }
    let mut args: Vec<OsString> = vec![
        "-c".into(),
        format!("remote.{}.mirror=false", request.remote).into(),
    ];
    args.extend(
        [
            "push",
            "--porcelain",
            "--progress",
            "--no-follow-tags",
            "--no-mirror",
        ]
        .into_iter()
        .map(OsString::from),
    );
    if request.force {
        let expected = request
            .expected
            .as_deref()
            .filter(|oid| valid_oid(oid))
            .ok_or("请先读取目标分支的远端提交；多个推送地址必须具有相同的目标提交")?;
        args.push(format!("--force-with-lease={target}:{expected}").into());
    }
    args.extend([
        OsString::from("--"),
        request.remote.clone().into(),
        format!("{}:{target}", request.source_oid).into(),
    ]);
    // 使用已确认的提交编号，外部切换分支或移动引用不会扩大本次推送范围。
    let output = git.run(args)?;
    if request.set_upstream {
        let local = local.ok_or("本地分支缺失")?;
        for (key, value) in [
            (format!("branch.{local}.remote"), request.remote.as_str()),
            (format!("branch.{local}.merge"), target.as_str()),
        ] {
            git.run(["config", "--local", "--replace-all", &key, value])
                .map_err(|error| format!("推送已完成，但设置上游失败：{error}"))?;
        }
    }
    Ok(git::redact(&format!(
        "已推送 {} → {}/{}\n{}",
        local.unwrap_or("HEAD"),
        request.remote,
        request.target,
        String::from_utf8_lossy(&output).trim()
    )))
}
