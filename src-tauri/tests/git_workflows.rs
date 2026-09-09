use gitgui_lib::{
    actions, ai, commit, conflict,
    git::{self, Git},
    model::*,
    push, repository,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Fixture {
    _dir: tempfile::TempDir,
    git: Git,
}
impl Fixture {
    fn push_request(&self, remote: &str, source: &str, target: &str) -> push::PushRequest {
        let info = push::remote_info(&self.git, remote).unwrap();
        push::PushRequest {
            remote: remote.into(),
            configuration: info.configuration,
            source: source.into(),
            source_oid: repository::resolve_commit(&self.git, source).unwrap(),
            target: target.into(),
            force: false,
            expected: None,
            set_upstream: source != "HEAD",
        }
    }
    fn push(&self, remote: &str, target: &str, expected: Option<&str>) -> git::Result<String> {
        let branch = self.git.text(["symbolic-ref", "HEAD"])?;
        let mut request = self.push_request(remote, &branch, target);
        request.force = expected.is_some();
        request.expected = expected.map(String::from);
        push::execute(&self.git, &request)
    }
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut git = Git::new(dir.path());
        git.context.helper = PathBuf::from(env!("CARGO_BIN_EXE_gitgui"));
        git.run(["init", "-b", "main"]).unwrap();
        git.run(["config", "user.name", "GitGUI 测试"]).unwrap();
        git.run(["config", "user.email", "gitgui-test@example.invalid"])
            .unwrap();
        git.run(["config", "commit.gpgsign", "false"]).unwrap();
        git.run(["config", "core.autocrlf", "false"]).unwrap();
        git.run(["config", "core.hooksPath", ".git/hooks"]).unwrap();
        Self { _dir: dir, git }
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.git.root.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }
    fn commit_all(&self, message: &str) {
        self.git.run(["add", "-A"]).unwrap();
        self.git.run(["commit", "-m", message]).unwrap();
    }
    fn initial(&self) {
        self.write(
            "a.txt",
            "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n",
        );
        self.write("b.txt", "original\n");
        self.commit_all("初始提交");
    }
    fn request(&self, selections: Vec<Selection>) -> CommitRequest {
        CommitRequest {
            snapshot: repository::status(&self.git).unwrap().snapshot,
            selections,
            message: "测试所选提交".into(),
            amend: false,
            push: false,
        }
    }
    fn full(&self, path: &str) -> Selection {
        Selection {
            file_id: repository::file_id(path.as_bytes()),
            all: true,
            line_ids: vec![],
            content_hash: None,
        }
    }
    fn partial(&self, path: &str, needle: &str) -> Selection {
        let id = repository::file_id(path.as_bytes());
        let diff = repository::diff(&self.git, &id, None, None, false).unwrap();
        let line_ids = diff
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|line| line.kind != "equal" && line.text.contains(needle))
            .map(|l| l.id.clone())
            .collect();
        Selection {
            file_id: id,
            all: false,
            line_ids,
            content_hash: Some(diff.content_hash),
        }
    }
    fn action(&self, kind: &str, args: &[&str]) -> git::Result<String> {
        actions::execute(
            &self.git,
            &GitAction {
                kind: kind.into(),
                args: args.iter().map(|s| s.to_string()).collect(),
                confirmed: true,
                snapshot: None,
            },
        )
    }
}

#[test]
fn initial_commit_and_special_names() {
    let f = Fixture::new();
    let path = "目录/空 格\n文件.txt";
    f.write(path, "新内容\n");
    let state = repository::status(&f.git).unwrap();
    assert!(state.head.is_none());
    assert_eq!(state.files[0].path, path);
    commit::commit(&f.git, &f.request(vec![f.full(path)])).unwrap();
    assert!(repository::status(&f.git).unwrap().files.is_empty());
    assert_eq!(
        repository::blob(&f.git, "HEAD", Path::new(path)).unwrap(),
        "新内容\n".as_bytes()
    );
}

fn isolated_ignore(f: &Fixture) {
    let path = f.git.root.join(".git/empty-excludes");
    fs::write(&path, "").unwrap();
    f.git
        .run(["config", "core.excludesFile", path.to_str().unwrap()])
        .unwrap();
}

#[test]
fn ignore_intent_to_add_clears_only_selected_markers_and_preserves_staged_versions() {
    let f = Fixture::new();
    f.initial();
    isolated_ignore(&f);
    f.write("b.txt", "已暂存版本\n");
    f.git.run(["add", "b.txt"]).unwrap();
    f.write("b.txt", "尚未暂存版本\n");
    for name in [".DS_Store", "deploy/.DS_Store", "keep-intent.txt"] {
        f.write(name, "本地保留\n");
    }
    f.git
        .run([
            "add",
            "-N",
            "--",
            ".DS_Store",
            "deploy/.DS_Store",
            "keep-intent.txt",
        ])
        .unwrap();
    f.git.run(["update-index", "--split-index"]).unwrap();
    let before = repository::status(&f.git).unwrap();
    assert!(
        before
            .files
            .iter()
            .find(|file| file.path == ".DS_Store")
            .unwrap()
            .intent_to_add
    );
    f.action(
        "ignorePaths",
        &[
            &repository::file_id(b".DS_Store"),
            &repository::file_id(b"deploy/.DS_Store"),
        ],
    )
    .unwrap();
    let after = repository::status(&f.git).unwrap();
    assert_eq!(after.head, before.head);
    assert!(!after
        .files
        .iter()
        .any(|file| file.path.ends_with(".DS_Store")));
    assert!(
        after
            .files
            .iter()
            .find(|file| file.path == "keep-intent.txt")
            .unwrap()
            .intent_to_add
    );
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "已暂存版本");
    assert_eq!(
        fs::read_to_string(f.git.root.join("b.txt")).unwrap(),
        "尚未暂存版本\n"
    );
    for name in [".DS_Store", "deploy/.DS_Store"] {
        assert_eq!(
            fs::read_to_string(f.git.root.join(name)).unwrap(),
            "本地保留\n"
        );
    }
}

#[test]
fn ignore_intent_to_add_in_unborn_repository_and_rejects_real_staging() {
    let f = Fixture::new();
    isolated_ignore(&f);
    f.write(".DS_Store", "local\n");
    f.git.run(["add", "-N", "--", ".DS_Store"]).unwrap();
    f.action("ignoreNames", &[&repository::file_id(b".DS_Store")])
        .unwrap();
    assert!(repository::head(&f.git).unwrap().is_none());
    assert!(!repository::status(&f.git)
        .unwrap()
        .files
        .iter()
        .any(|file| file.path == ".DS_Store"));
    f.write("real.txt", "real staged\n");
    f.git.run(["add", "--", "real.txt"]).unwrap();
    let rules = fs::read(f.git.root.join(".gitignore")).unwrap();
    assert!(f
        .action("ignorePaths", &[&repository::file_id(b"real.txt")])
        .is_err());
    assert_eq!(fs::read(f.git.root.join(".gitignore")).unwrap(), rules);
    assert_eq!(f.git.text(["show", ":real.txt"]).unwrap(), "real staged");
}

#[test]
fn ignore_exact_paths_preserves_files_index_existing_rules_and_crlf() {
    let f = Fixture::new();
    f.initial();
    isolated_ignore(&f);
    f.write(".gitignore", "# 现有说明\r\nbuild/\r\n");
    f.git.run(["add", ".gitignore"]).unwrap();
    f.write(".gitignore", "# 现有说明\r\nbuild/\r\n# 尚未暂存的说明");
    let previous = fs::read(f.git.root.join(".gitignore")).unwrap();
    let index = fs::read(f.git.root.join(".git/index")).unwrap();
    for path in [".DS_Store", "deploy/.DS_Store", "docs/.DS_Store"] {
        f.write(path, "保留原文件\n");
    }
    f.action(
        "ignorePaths",
        &[
            &repository::file_id(b".DS_Store"),
            &repository::file_id(b"deploy/.DS_Store"),
        ],
    )
    .unwrap();
    let rules = fs::read(f.git.root.join(".gitignore")).unwrap();
    assert!(rules.starts_with(&previous));
    assert!(rules.ends_with(b"\r\n/.DS_Store\r\n/deploy/.DS_Store\r\n"));
    assert_eq!(fs::read(f.git.root.join(".git/index")).unwrap(), index);
    for path in [".DS_Store", "deploy/.DS_Store", "docs/.DS_Store"] {
        assert_eq!(
            fs::read_to_string(f.git.root.join(path)).unwrap(),
            "保留原文件\n"
        );
    }
    let state = repository::status(&f.git).unwrap();
    assert!(!state
        .files
        .iter()
        .any(|file| file.path == ".DS_Store" || file.path == "deploy/.DS_Store"));
    assert!(state.files.iter().any(|file| file.path == "docs/.DS_Store"));
}

#[test]
fn ignore_by_name_covers_nested_untracked_files_but_keeps_tracked_changes() {
    let f = Fixture::new();
    f.initial();
    isolated_ignore(&f);
    f.write("tracked/.DS_Store", "tracked\n");
    f.commit_all("跟踪一个同名文件");
    f.write("tracked/.DS_Store", "tracked modified\n");
    for path in [".DS_Store", "one/.DS_Store", "one/two/.DS_Store"] {
        f.write(path, "local\n");
    }
    f.action(
        "ignoreNames",
        &[
            &repository::file_id(b".DS_Store"),
            &repository::file_id(b"one/.DS_Store"),
        ],
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(f.git.root.join(".gitignore")).unwrap(),
        ".DS_Store\n"
    );
    let state = repository::status(&f.git).unwrap();
    assert!(!state
        .files
        .iter()
        .any(|file| file.untracked && file.path.ends_with(".DS_Store")));
    assert!(state
        .files
        .iter()
        .any(|file| file.path == "tracked/.DS_Store" && !file.untracked));
}

#[test]
fn ignore_special_names_are_literal_and_do_not_hide_similar_names() {
    let f = Fixture::new();
    f.initial();
    isolated_ignore(&f);
    let names = [
        "目录/空 格[1]*?.txt ",
        "!important",
        "#note",
        "a*b",
        "a?b",
        "a[b]",
        "back\\slash",
        " space ",
    ];
    for path in names {
        f.write(path, "literal\n");
    }
    for path in ["axb", "ab", "backslash", "space", "目录/空 格1XX.txt"] {
        f.write(path, "keep visible\n");
    }
    let ids: Vec<_> = names
        .iter()
        .map(|name| repository::file_id(name.as_bytes()))
        .collect();
    f.action(
        "ignorePaths",
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .unwrap();
    let state = repository::status(&f.git).unwrap();
    for name in names {
        assert!(!state.files.iter().any(|file| file.path == name), "{name}");
    }
    for name in ["axb", "ab", "backslash", "space", "目录/空 格1XX.txt"] {
        assert!(state.files.iter().any(|file| file.path == name), "{name}");
    }
}

#[test]
fn ignore_rejects_tracked_files_newline_names_and_stale_snapshots_without_writing() {
    let f = Fixture::new();
    f.initial();
    isolated_ignore(&f);
    f.write("local.txt", "local\n");
    f.write("a.txt", "tracked modification\n");
    f.write(".gitignore", "# keep\n");
    let index = fs::read(f.git.root.join(".git/index")).unwrap();
    assert!(f
        .action(
            "ignorePaths",
            &[
                &repository::file_id(b"local.txt"),
                &repository::file_id(b"a.txt")
            ]
        )
        .is_err());
    f.write("line\nbreak", "local\n");
    assert!(f
        .action("ignorePaths", &[&repository::file_id(b"line\nbreak")])
        .is_err());
    let snapshot = repository::status(&f.git).unwrap().snapshot;
    f.git.run(["add", "local.txt"]).unwrap();
    let result = actions::execute(
        &f.git,
        &GitAction {
            kind: "ignorePaths".into(),
            args: vec![repository::file_id(b"local.txt")],
            confirmed: false,
            snapshot: Some(snapshot),
        },
    );
    assert!(result.unwrap_err().contains("仓库状态已变化"));
    assert_eq!(
        fs::read_to_string(f.git.root.join(".gitignore")).unwrap(),
        "# keep\n"
    );
    assert_ne!(fs::read(f.git.root.join(".git/index")).unwrap(), index);
}

#[cfg(unix)]
#[test]
fn ignore_protects_symlinks_and_external_index_locks() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let f = Fixture::new();
    f.initial();
    isolated_ignore(&f);
    f.write("local.txt", "local\n");
    let outside = tempfile::tempdir().unwrap();
    let protected = outside.path().join("protected");
    fs::write(&protected, "untouched").unwrap();
    symlink(&protected, f.git.root.join(".gitignore")).unwrap();
    assert!(f
        .action("ignorePaths", &[&repository::file_id(b"local.txt")])
        .is_err());
    assert_eq!(fs::read_to_string(protected).unwrap(), "untouched");
    let g = Fixture::new();
    g.initial();
    isolated_ignore(&g);
    let path = std::ffi::OsStr::from_bytes(b"raw.txt");
    fs::write(g.git.root.join(path), "raw\n").unwrap();
    let lock = g.git.path("index").unwrap().with_extension("lock");
    fs::write(&lock, "existing lock").unwrap();
    assert!(g
        .action("ignorePaths", &[&repository::file_id(path.as_bytes())])
        .is_err());
    assert_eq!(fs::read_to_string(&lock).unwrap(), "existing lock");
    fs::remove_file(&lock).unwrap();
    g.action("ignorePaths", &[&repository::file_id(path.as_bytes())])
        .unwrap();
    assert_eq!(
        fs::read(g.git.root.join(".gitignore")).unwrap(),
        b"/raw.txt\n"
    );
    assert!(!repository::status(&g.git)
        .unwrap()
        .files
        .iter()
        .any(|file| file.id == repository::file_id(path.as_bytes())));
}

#[test]
fn ignore_worktree_rules_stay_in_worktree_and_nested_reinclude_is_reported() {
    let f = Fixture::new();
    f.initial();
    isolated_ignore(&f);
    let outer = tempfile::tempdir().unwrap();
    let worktree = outer.path().join("side");
    f.git
        .run(vec![
            "worktree".into(),
            "add".into(),
            "-b".into(),
            "side".into(),
            worktree.clone().into_os_string(),
        ])
        .unwrap();
    fs::write(worktree.join(".DS_Store"), "local").unwrap();
    let side = Git::new(&worktree);
    actions::execute(
        &side,
        &GitAction {
            kind: "ignorePaths".into(),
            args: vec![repository::file_id(b".DS_Store")],
            confirmed: false,
            snapshot: None,
        },
    )
    .unwrap();
    assert!(worktree.join(".gitignore").is_file());
    assert!(!f.git.root.join(".gitignore").exists());
    f.write("child/.gitignore", "!.DS_Store\n");
    f.write("child/.DS_Store", "re-included\n");
    let result = f
        .action("ignorePaths", &[&repository::file_id(b"child/.DS_Store")])
        .unwrap();
    assert!(result.contains("仍有 1 项未被忽略"));
    assert!(repository::status(&f.git)
        .unwrap()
        .files
        .iter()
        .any(|file| file.path == "child/.DS_Store"));
}

fn bare_copy(f: &Fixture, name: &str) -> PathBuf {
    let parent = f.git.root.join(".git/test-remotes");
    fs::create_dir_all(&parent).unwrap();
    let path = parent.join(name);
    f.git
        .run(vec![
            "clone".into(),
            "--bare".into(),
            "--".into(),
            f.git.root.clone().into_os_string(),
            path.clone().into_os_string(),
        ])
        .unwrap();
    path
}

#[test]
fn push_lists_all_actual_push_url_branches_without_local_tracking_refs() {
    let f = Fixture::new();
    f.initial();
    let fetch = bare_copy(&f, "fetch.git");
    let destination = bare_copy(&f, "push.git");
    f.git
        .run(["remote", "add", "origin", fetch.to_str().unwrap()])
        .unwrap();
    f.git
        .run([
            "remote",
            "set-url",
            "--push",
            "origin",
            destination.to_str().unwrap(),
        ])
        .unwrap();
    let oid = repository::head(&f.git).unwrap().unwrap();
    fs::create_dir_all(destination.join("refs/heads/topic")).unwrap();
    for index in 0..240 {
        fs::write(
            destination.join(format!("refs/heads/topic/feature-{index:03}")),
            format!("{oid}\n"),
        )
        .unwrap();
    }
    fs::write(fetch.join("refs/heads/fetch-only"), format!("{oid}\n")).unwrap();
    let before = repository::refs(&f.git).unwrap();
    assert!(!before.iter().any(|r| r.kind == "remote"));
    let info = push::remote_info(&f.git, "origin").unwrap();
    assert_eq!(info.branches.len(), 241);
    assert!(info.branches.iter().any(|b| b.name == "topic/feature-239"));
    assert!(!info.branches.iter().any(|b| b.name == "fetch-only"));
    assert_eq!(repository::refs(&f.git).unwrap().len(), before.len());
}

#[test]
fn push_selected_noncurrent_branch_sets_only_its_upstream_and_does_not_push_tags() {
    let f = Fixture::new();
    f.initial();
    let destination = bare_copy(&f, "remote.git");
    f.git
        .run(["remote", "add", "origin", destination.to_str().unwrap()])
        .unwrap();
    let old = repository::head(&f.git).unwrap().unwrap();
    f.git.run(["branch", "release/candidate"]).unwrap();
    f.write("a.txt", "new main\n");
    f.commit_all("新的主分支提交");
    let main = repository::head(&f.git).unwrap().unwrap();
    f.git
        .run(["tag", "-a", "private-tag", "-m", "不在本次推送范围"])
        .unwrap();
    f.git.run(["config", "push.followTags", "true"]).unwrap();
    f.git
        .run(["config", "remote.origin.mirror", "true"])
        .unwrap();
    let request = f.push_request("origin", "refs/heads/release/candidate", "review/selected");
    push::execute(&f.git, &request).unwrap();
    assert_eq!(
        Git::new(&destination)
            .text(["rev-parse", "refs/heads/review/selected"])
            .unwrap(),
        old
    );
    assert_eq!(repository::head(&f.git).unwrap().unwrap(), main);
    assert_eq!(
        f.git
            .text(["config", "branch.release/candidate.remote"])
            .unwrap(),
        "origin"
    );
    assert_eq!(
        f.git
            .text(["config", "branch.release/candidate.merge"])
            .unwrap(),
        "refs/heads/review/selected"
    );
    assert_eq!(
        f.git
            .text(["rev-parse", "release/candidate@{upstream}"])
            .unwrap(),
        old
    );
    assert_ne!(
        Git::new(&destination)
            .raw(["show-ref", "--verify", "refs/tags/private-tag"])
            .unwrap()
            .code,
        0
    );
    assert_eq!(
        f.git
            .raw(["config", "--get", "branch.main.remote"])
            .unwrap()
            .code,
        1
    );
}

#[test]
fn push_rejects_source_or_destination_configuration_changed_since_confirmation() {
    let f = Fixture::new();
    f.initial();
    let destination = bare_copy(&f, "remote.git");
    f.git
        .run(["remote", "add", "origin", destination.to_str().unwrap()])
        .unwrap();
    let request = f.push_request("origin", "refs/heads/main", "review/new");
    f.write("a.txt", "new source\n");
    f.commit_all("外部变更");
    assert!(push::execute(&f.git, &request)
        .unwrap_err()
        .contains("本地分支提交已变化"));
    let request = f.push_request("origin", "refs/heads/main", "review/new");
    let other = bare_copy(&f, "other.git");
    f.git
        .run([
            "remote",
            "set-url",
            "--push",
            "origin",
            other.to_str().unwrap(),
        ])
        .unwrap();
    assert!(push::execute(&f.git, &request)
        .unwrap_err()
        .contains("推送地址已变化"));
    assert_ne!(
        Git::new(&destination)
            .raw(["show-ref", "--verify", "refs/heads/review/new"])
            .unwrap()
            .code,
        0
    );
}

#[test]
fn push_multiple_destinations_exposes_union_and_inconsistent_force_leases() {
    let f = Fixture::new();
    f.initial();
    let one = bare_copy(&f, "one.git");
    f.write("a.txt", "new second destination\n");
    f.commit_all("另一端提交");
    let two = bare_copy(&f, "two.git");
    f.git
        .run(["remote", "add", "origin", one.to_str().unwrap()])
        .unwrap();
    f.git
        .run([
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            one.to_str().unwrap(),
        ])
        .unwrap();
    f.git
        .run([
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            two.to_str().unwrap(),
        ])
        .unwrap();
    let oid = repository::head(&f.git).unwrap().unwrap();
    fs::write(two.join("refs/heads/only-two"), format!("{oid}\n")).unwrap();
    let info = push::remote_info(&f.git, "origin").unwrap();
    assert_eq!(info.urls.len(), 2);
    assert!(info
        .branches
        .iter()
        .find(|b| b.name == "main")
        .unwrap()
        .oid
        .is_none());
    assert_eq!(
        info.branches
            .iter()
            .find(|b| b.name == "only-two")
            .unwrap()
            .destinations,
        1
    );
    let mut request = f.push_request("origin", "refs/heads/main", "new-target");
    request.force = true;
    assert!(push::execute(&f.git, &request).is_err());
    request.force = false;
    push::execute(&f.git, &request).unwrap();
    for path in [&one, &two] {
        assert_eq!(
            Git::new(path)
                .text(["rev-parse", "refs/heads/new-target"])
                .unwrap(),
            oid
        );
    }
}

#[test]
fn ai_partial_input_excludes_unselected_content_and_preserves_repository() {
    let f = Fixture::new();
    f.initial();
    f.write("b.txt", "UNSELECTED_STAGED_SECRET\n");
    f.git.run(["add", "b.txt"]).unwrap();
    f.write("a.txt", "one\nselected addition\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nUNSELECTED_WORKTREE_SECRET\n");
    let selection = f.partial("a.txt", "selected addition");
    let before = repository::status(&f.git).unwrap();
    let index = fs::read(f.git.root.join(".git/index")).unwrap();
    let work = fs::read(f.git.root.join("a.txt")).unwrap();
    let input = ai::prepare(&f.git, &before.snapshot, &[selection]).unwrap();
    assert!(input.contains("selected addition"));
    assert!(!input.contains("UNSELECTED_"));
    assert!(!input.contains("b.txt"));
    assert_eq!(fs::read(f.git.root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(f.git.root.join("a.txt")).unwrap(), work);
    assert_eq!(repository::status(&f.git).unwrap().head, before.head);
}

#[test]
fn ai_rejects_stale_snapshot_and_selection_and_oversized_input() {
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "selected addition\n");
    let selection = f.partial("a.txt", "selected addition");
    let snapshot = repository::status(&f.git).unwrap().snapshot;
    f.write("a.txt", "new external content\n");
    assert!(
        ai::prepare(&f.git, &snapshot, std::slice::from_ref(&selection))
            .unwrap_err()
            .contains("变化")
    );
    let current = repository::status(&f.git).unwrap();
    assert!(ai::prepare(&f.git, &current.snapshot, &[selection])
        .unwrap_err()
        .contains("代码块已变化"));
    f.write("large.txt", &"新内容".repeat(20000));
    let state = repository::status(&f.git).unwrap();
    assert!(ai::prepare(&f.git, &state.snapshot, &[f.full("large.txt")])
        .unwrap_err()
        .contains("128 KB"));
    assert!(ai::prepare(&f.git, &state.snapshot, &[]).is_err());
}

#[test]
fn ai_handles_empty_repository_special_names_binary_and_large_summary() {
    let f = Fixture::new();
    let name = "中文 空格\n新文件.txt";
    f.write(name, "新增功能\n");
    fs::write(f.git.root.join("binary.dat"), [0, 1, 2]).unwrap();
    f.write("large.txt", &"x".repeat(5 * 1024 * 1024 + 1));
    let state = repository::status(&f.git).unwrap();
    let input = ai::prepare(
        &f.git,
        &state.snapshot,
        &[f.full(name), f.full("binary.dat"), f.full("large.txt")],
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&input).unwrap();
    assert_eq!(value["selectedChanges"][0]["path"], name);
    assert!(value["selectedChanges"][0]["content"]["patch"]
        .as_str()
        .unwrap()
        .contains("+新增功能"));
    assert!(value["selectedChanges"][1]["content"]["summary"].is_string());
    assert!(value["selectedChanges"][2]["content"]["summary"].is_string());
    assert!(input.len() < 2048);
}

#[test]
fn partial_preserves_unrelated_staged_and_worktree() {
    let f = Fixture::new();
    f.initial();
    f.write("b.txt", "staged elsewhere\n");
    f.git.run(["add", "b.txt"]).unwrap();
    f.write(
        "a.txt",
        "one\nadded first\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nadded second\n",
    );
    let before_worktree = fs::read(f.git.root.join("a.txt")).unwrap();
    commit::commit(&f.git, &f.request(vec![f.partial("a.txt", "added first")])).unwrap();
    let head = f.git.text(["show", "HEAD:a.txt"]).unwrap();
    assert!(head.contains("added first"));
    assert!(!head.contains("added second"));
    assert_eq!(fs::read(f.git.root.join("a.txt")).unwrap(), before_worktree);
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "staged elsewhere");
    assert_eq!(f.git.text(["show", "HEAD:b.txt"]).unwrap(), "original");
    assert!(f
        .git
        .text(["diff", "--cached", "--name-only"])
        .unwrap()
        .contains("b.txt"));
}

#[test]
fn partial_preserves_staged_changes_in_same_file() {
    let f = Fixture::new();
    f.initial();
    f.write(
        "a.txt",
        "one\nstaged first\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\n",
    );
    f.git.run(["add", "a.txt"]).unwrap();
    f.write(
        "a.txt",
        "one\nstaged first\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nselected last\n",
    );
    commit::commit(
        &f.git,
        &f.request(vec![f.partial("a.txt", "selected last")]),
    )
    .unwrap();
    let head = f.git.text(["show", "HEAD:a.txt"]).unwrap();
    assert!(head.contains("selected last"));
    assert!(!head.contains("staged first"));
    let index = f.git.text(["show", ":a.txt"]).unwrap();
    assert!(index.contains("selected last"));
    assert!(index.contains("staged first"));
}

#[test]
fn overlapping_staged_version_is_rejected_before_commit() {
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "staged conflict\n");
    f.git.run(["add", "a.txt"]).unwrap();
    f.write("a.txt", "different worktree\n");
    let head = repository::head(&f.git).unwrap();
    let index = repository::index_bytes(&f.git).unwrap();
    let error = commit::commit(&f.git, &f.request(vec![f.full("a.txt")])).unwrap_err();
    assert!(error.contains("重叠") || error.contains("冲突"));
    assert_eq!(head, repository::head(&f.git).unwrap());
    assert_eq!(index, repository::index_bytes(&f.git).unwrap());
}

#[test]
fn stale_snapshot_and_external_lock_are_rejected() {
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "selection\n");
    let request = f.request(vec![f.full("a.txt")]);
    f.write("a.txt", "external modification\n");
    assert!(commit::commit(&f.git, &request)
        .unwrap_err()
        .contains("变化"));
    let request = f.request(vec![f.full("a.txt")]);
    fs::write(
        f.git.path("index").unwrap().with_extension("lock"),
        "external",
    )
    .unwrap();
    assert!(commit::commit(&f.git, &request)
        .unwrap_err()
        .contains("锁定"));
}

#[cfg(unix)]
fn hook(f: &Fixture, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = f.git.root.join(".git/hooks/pre-commit");
    fs::write(&path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[cfg(unix)]
fn hook_failure_preserves_original_index() {
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "new\n");
    f.write("b.txt", "existing staged\n");
    f.git.run(["add", "b.txt"]).unwrap();
    hook(&f, "#!/bin/sh\necho hook-rejected >&2\nexit 1\n");
    let before = repository::index_bytes(&f.git).unwrap();
    let head = repository::head(&f.git).unwrap();
    assert!(commit::commit(&f.git, &f.request(vec![f.full("a.txt")]))
        .unwrap_err()
        .contains("hook-rejected"));
    assert_eq!(before, repository::index_bytes(&f.git).unwrap());
    assert_eq!(head, repository::head(&f.git).unwrap());
}

#[test]
#[cfg(unix)]
fn hook_cannot_expand_selected_commit() {
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "selected\n");
    f.write("b.txt", "unselected\n");
    hook(&f, "#!/bin/sh\ngit add b.txt\n");
    let head = repository::head(&f.git).unwrap();
    let error = commit::commit(&f.git, &f.request(vec![f.full("a.txt")])).unwrap_err();
    assert!(error.contains("钩子改变"), "{error}");
    assert_eq!(head, repository::head(&f.git).unwrap());
    assert!(f
        .git
        .text(["diff", "--cached", "--name-only"])
        .unwrap()
        .is_empty());
}

#[test]
fn rename_delete_and_binary_whole_file_commit() {
    let f = Fixture::new();
    f.initial();
    f.git.run(["mv", "a.txt", "改名.txt"]).unwrap();
    fs::remove_file(f.git.root.join("b.txt")).unwrap();
    fs::write(f.git.root.join("binary.bin"), [0, 255, 13, 10]).unwrap();
    commit::commit(
        &f.git,
        &f.request(vec![
            f.full("改名.txt"),
            f.full("b.txt"),
            f.full("binary.bin"),
        ]),
    )
    .unwrap();
    assert!(repository::status(&f.git).unwrap().files.is_empty());
    assert_eq!(
        f.git.run(["show", "HEAD:binary.bin"]).unwrap(),
        [0, 255, 13, 10]
    );
}

#[test]
fn intent_to_add_and_flags_survive_unrelated_commit() {
    let f = Fixture::new();
    f.initial();
    f.write("future.txt", "future\n");
    f.git.run(["add", "-N", "future.txt"]).unwrap();
    f.git
        .run(["update-index", "--assume-unchanged", "b.txt"])
        .unwrap();
    f.write("a.txt", "selected\n");
    commit::commit(&f.git, &f.request(vec![f.full("a.txt")])).unwrap();
    assert!(!f
        .git
        .text(["ls-tree", "--name-only", "HEAD"])
        .unwrap()
        .contains("future.txt"));
    assert!(f
        .git
        .text(["ls-files", "-v", "b.txt"])
        .unwrap()
        .starts_with('h'));
    assert!(repository::status(&f.git)
        .unwrap()
        .files
        .iter()
        .any(|f| f.path == "future.txt"));
}

#[test]
fn split_index_survives_commit_semantics() {
    let f = Fixture::new();
    f.initial();
    f.git.run(["update-index", "--split-index"]).unwrap();
    f.write("b.txt", "staged\n");
    f.git.run(["add", "b.txt"]).unwrap();
    f.write("a.txt", "selected\n");
    commit::commit(&f.git, &f.request(vec![f.full("a.txt")])).unwrap();
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "staged");
    assert_eq!(f.git.text(["show", "HEAD:b.txt"]).unwrap(), "original");
}

#[test]
fn merge_conflict_save_mark_and_continue() {
    let f = Fixture::new();
    f.initial();
    f.action("branchCreate", &["topic"]).unwrap();
    f.write("a.txt", "topic content\n");
    f.commit_all("主题分支");
    f.action("switch", &["main"]).unwrap();
    f.write("a.txt", "main content\n");
    f.commit_all("主分支");
    assert!(f.action("merge", &["topic"]).is_err());
    let id = repository::file_id(b"a.txt");
    let file = conflict::read(&f.git, &id).unwrap();
    assert!(file.ours.unwrap().contains("main content"));
    assert!(file.theirs.unwrap().contains("topic content"));
    let hash = conflict::save(&f.git, &id, &file.result_hash, "merged result\n").unwrap();
    assert!(repository::status(&f.git).unwrap().files[0].conflict);
    conflict::mark(&f.git, &id, &hash).unwrap();
    // 合并说明由 Git 标准配置提供，不在测试中打开 GUI 编辑器。
    let mut git = f.git.clone();
    git.extra_env.push(("GIT_EDITOR".into(), "true".into()));
    actions::execute(
        &git,
        &GitAction {
            kind: "continue".into(),
            args: vec![],
            confirmed: true,
            snapshot: None,
        },
    )
    .unwrap();
    assert!(repository::status(&f.git).unwrap().operation.is_none());
    assert_eq!(f.git.text(["show", "HEAD:a.txt"]).unwrap(), "merged result");
}

#[test]
fn rebase_conflict_labels_and_abort() {
    let f = Fixture::new();
    f.initial();
    f.action("branchCreate", &["topic"]).unwrap();
    f.write("a.txt", "topic\n");
    f.commit_all("topic");
    let topic_head = repository::head(&f.git).unwrap();
    f.action("switch", &["main"]).unwrap();
    f.write("a.txt", "upstream\n");
    f.commit_all("main");
    f.action("switch", &["topic"]).unwrap();
    assert!(f.action("rebase", &["main"]).is_err());
    let conflict = conflict::read(&f.git, &repository::file_id(b"a.txt")).unwrap();
    assert!(conflict.ours_label.contains("变基目标"));
    assert!(conflict.ours.unwrap().contains("upstream"));
    assert!(conflict.theirs.unwrap().contains("topic"));
    f.action("abort", &[]).unwrap();
    assert_eq!(repository::head(&f.git).unwrap(), topic_head);
}

#[test]
fn delete_modify_conflict_can_preserve_or_delete() {
    let f = Fixture::new();
    f.initial();
    f.action("branchCreate", &["topic"]).unwrap();
    f.write("a.txt", "changed\n");
    f.commit_all("change");
    f.action("switch", &["main"]).unwrap();
    fs::remove_file(f.git.root.join("a.txt")).unwrap();
    f.commit_all("delete");
    assert!(f.action("merge", &["topic"]).is_err());
    let id = repository::file_id(b"a.txt");
    let file = conflict::read(&f.git, &id).unwrap();
    assert!(!file.ours_exists && file.theirs_exists);
    conflict::choose(&f.git, &id, "ours", &file.result_hash).unwrap();
    let current = conflict::read(&f.git, &id).unwrap();
    conflict::mark(&f.git, &id, &current.result_hash).unwrap();
    assert!(!f.git.root.join("a.txt").exists());
    f.action("abort", &[]).unwrap();
}

#[test]
fn stash_worktree_history_and_local_remote() {
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "temporary\n");
    f.write("new.txt", "untracked\n");
    f.action("stashCreate", &["切换任务", "true", "false"])
        .unwrap();
    let state = repository::status(&f.git).unwrap();
    assert!(state.files.is_empty(), "{:?}", state.files);
    let stashes = actions::collections(&f.git, "stash").unwrap();
    assert_eq!(stashes.as_array().unwrap().len(), 1);
    f.action("stashPop", &["stash@{0}", "false"]).unwrap();
    assert_eq!(
        fs::read_to_string(f.git.root.join("new.txt")).unwrap(),
        "untracked\n"
    );
    f.commit_all("恢复暂存工作");
    let history = repository::history(
        &f.git,
        &HistoryQuery {
            skip: 0,
            limit: 200,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].subject, "恢复暂存工作");
    let outer = tempfile::tempdir().unwrap();
    let tree = outer.path().join("work tree");
    f.action("worktreeAdd", &[tree.to_str().unwrap(), "parallel", "new"])
        .unwrap();
    assert_eq!(
        actions::collections(&f.git, "worktree")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let opened = repository::open(tree.to_str().unwrap()).unwrap();
    assert_ne!(opened.git_dir, opened.common_dir);
    f.action("worktreeRemove", &[tree.to_str().unwrap()])
        .unwrap();
    let remote = outer.path().join("remote.git");
    Git::new(outer.path())
        .run(vec![
            "init".into(),
            "--bare".into(),
            remote.clone().into_os_string(),
        ])
        .unwrap();
    f.action("remoteAdd", &["origin", remote.to_str().unwrap()])
        .unwrap();
    assert_eq!(repository::remotes(&f.git).unwrap().len(), 1);
    f.push("origin", "main", None).unwrap();
    assert_eq!(
        Git::new(&remote)
            .text(["rev-parse", "refs/heads/main"])
            .unwrap(),
        repository::head(&f.git).unwrap().unwrap()
    );
}

#[test]
fn path_escape_and_conflict_markers_are_rejected() {
    assert!(repository::decode_path(&repository::file_id(b"../outside")).is_err());
    assert!(repository::decode_path(&repository::file_id(b".git/config")).is_err());
    assert!(conflict::has_markers(
        "<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n"
    ));
    assert!(!conflict::has_markers("普通文本 <<<<<< 不是标记\n"));
}

#[test]
fn history_diff_handles_root_rename_delete_and_literal_paths() {
    let f = Fixture::new();
    let special = ":(glob)[special]*.txt";
    f.write(special, "literal\n");
    f.write("other.txt", "other\n");
    f.commit_all("root");
    let first = repository::head(&f.git).unwrap().unwrap();
    let files = repository::commit_files(&f.git, &first, None).unwrap();
    assert_eq!(files.len(), 2);
    let detail = repository::diff(
        &f.git,
        &repository::file_id(special.as_bytes()),
        None,
        Some(&first),
        false,
    )
    .unwrap();
    assert_eq!(detail.old_text.unwrap(), "");
    assert_eq!(detail.new_text.unwrap(), "literal\n");
    f.git.run(["mv", "--", special, "renamed.txt"]).unwrap();
    f.commit_all("rename");
    let second = repository::head(&f.git).unwrap().unwrap();
    let diff = repository::diff(
        &f.git,
        &repository::file_id(b"renamed.txt"),
        None,
        Some(&second),
        false,
    )
    .unwrap();
    assert_eq!(diff.old_text, diff.new_text);
    f.write(special, "literal new\n");
    commit::commit(&f.git, &f.request(vec![f.full(special)])).unwrap();
    assert_eq!(f.git.text(["show", "HEAD:other.txt"]).unwrap(), "other");
    fs::remove_file(f.git.root.join("renamed.txt")).unwrap();
    f.commit_all("delete");
    let diff = repository::diff(
        &f.git,
        &repository::file_id(b"renamed.txt"),
        None,
        Some("HEAD"),
        false,
    )
    .unwrap();
    assert_eq!(diff.new_text.unwrap(), "");
    assert_eq!(diff.old_text.unwrap(), "literal\n");
}

#[test]
fn partial_preserves_crlf_and_missing_final_newline() {
    let f = Fixture::new();
    f.write("line.txt", "one\r\ntwo\r\nthree");
    f.commit_all("initial");
    f.write("line.txt", "one\r\ninserted\r\ntwo\r\nthree\r\nlater");
    commit::commit(&f.git, &f.request(vec![f.partial("line.txt", "inserted")])).unwrap();
    assert_eq!(
        f.git.run(["show", "HEAD:line.txt"]).unwrap(),
        b"one\r\ninserted\r\ntwo\r\nthree"
    );
    assert!(fs::read(f.git.root.join("line.txt"))
        .unwrap()
        .ends_with(b"later"));
}

#[test]
#[cfg(unix)]
fn symlinks_are_committed_as_links_and_escape_parents_are_rejected() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.initial();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("secret.txt"), "outside").unwrap();
    symlink(external.path(), f.git.root.join("outside-link")).unwrap();
    assert!(
        repository::safe_work_path(&f.git, &repository::file_id(b"outside-link/secret.txt"))
            .is_err()
    );
    symlink("a.txt", f.git.root.join("alias.txt")).unwrap();
    commit::commit(&f.git, &f.request(vec![f.full("alias.txt")])).unwrap();
    assert!(f
        .git
        .text(["ls-tree", "HEAD", "alias.txt"])
        .unwrap()
        .starts_with("120000"));
    assert_eq!(f.git.text(["show", "HEAD:alias.txt"]).unwrap(), "a.txt");
}

#[test]
fn cherry_pick_revert_tags_and_reset_preserve_expected_history() {
    let f = Fixture::new();
    f.initial();
    f.action("branchCreate", &["topic"]).unwrap();
    f.write("feature.txt", "feature\n");
    f.commit_all("feature");
    let feature = repository::head(&f.git).unwrap().unwrap();
    f.action("switch", &["main"]).unwrap();
    f.action("cherryPick", &[&feature]).unwrap();
    let picked = repository::head(&f.git).unwrap().unwrap();
    assert!(f.git.root.join("feature.txt").is_file());
    f.action("tagCreate", &["v1.1", &picked, "发布说明"])
        .unwrap();
    assert!(repository::refs(&f.git)
        .unwrap()
        .iter()
        .any(|r| r.name == "v1.1"));
    f.action("revert", &[&picked]).unwrap();
    assert!(!f.git.root.join("feature.txt").exists());
    f.action("reset", &["soft", &picked]).unwrap();
    assert!(!repository::status(&f.git).unwrap().files.is_empty());
    assert!(f.action("branchDelete", &["main"]).is_err());
}

#[test]
fn non_fast_forward_push_and_stale_force_lease_are_rejected() {
    let f = Fixture::new();
    f.initial();
    let outer = tempfile::tempdir().unwrap();
    let remote = outer.path().join("remote.git");
    Git::new(outer.path())
        .run(vec![
            "init".into(),
            "--bare".into(),
            remote.clone().into_os_string(),
        ])
        .unwrap();
    f.action("remoteAdd", &["origin", remote.to_str().unwrap()])
        .unwrap();
    f.push("origin", "main", None).unwrap();
    let old = repository::head(&f.git).unwrap().unwrap();
    let other_path = outer.path().join("other");
    Git::new(outer.path())
        .run(vec![
            "clone".into(),
            "--branch".into(),
            "main".into(),
            remote.clone().into_os_string(),
            other_path.clone().into_os_string(),
        ])
        .unwrap();
    let other = Git::new(&other_path);
    other.run(["config", "user.name", "Other Test"]).unwrap();
    other
        .run(["config", "user.email", "other@example.invalid"])
        .unwrap();
    other.run(["config", "commit.gpgsign", "false"]).unwrap();
    fs::write(other_path.join("other.txt"), "other work\n").unwrap();
    other.run(["add", "-A"]).unwrap();
    other.run(["commit", "-m", "other work"]).unwrap();
    other.run(["push"]).unwrap();
    let remote_head = repository::head(&other).unwrap().unwrap();
    f.write("local.txt", "local\n");
    f.commit_all("local work");
    assert!(f.push("origin", "main", None).is_err());
    assert!(f.push("origin", "main", Some(&old)).is_err());
    assert_eq!(
        Git::new(&remote)
            .text(["rev-parse", "refs/heads/main"])
            .unwrap(),
        remote_head
    );
}

#[test]
fn interactive_rebase_uses_controlled_sequence_and_message_helpers() {
    use gitgui_lib::helper;
    use std::sync::{Arc, Mutex};
    let f = Fixture::new();
    f.initial();
    f.write("second.txt", "second\n");
    f.commit_all("second");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let ask: helper::Ask = Arc::new(move |prompt, _| {
        recorded.lock().unwrap().push(prompt.kind.clone());
        if prompt.kind == "sequence" {
            Ok(prompt.content.replacen("pick ", "reword ", 1))
        } else if prompt.kind == "editor" {
            Ok("通过受控编辑器修改根提交".into())
        } else {
            Err("不应出现凭据请求".into())
        }
    });
    let bridge = helper::Bridge::start("test-rebase".into(), &f.git.context, ask).unwrap();
    let mut git = f.git.clone();
    git.context.env.extend(bridge.env.clone());
    actions::execute(
        &git,
        &GitAction {
            kind: "interactiveRebase".into(),
            args: vec!["--root".into()],
            confirmed: true,
            snapshot: None,
        },
    )
    .unwrap();
    let events = seen.lock().unwrap();
    assert!(events.iter().any(|kind| kind == "sequence"));
    assert!(events.iter().any(|kind| kind == "editor"));
    assert!(git
        .text(["log", "--format=%s"])
        .unwrap()
        .contains("通过受控编辑器修改根提交"));
    assert!(repository::status(&git).unwrap().operation.is_none());
}

#[test]
#[cfg(unix)]
fn post_commit_hook_staging_is_combined_with_existing_staging() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.initial();
    f.write("b.txt", "previous staged\n");
    f.git.run(["add", "b.txt"]).unwrap();
    f.write("a.txt", "selected\n");
    f.write("hook.txt", "hook addition\n");
    let path = f.git.root.join(".git/hooks/post-commit");
    fs::write(&path, "#!/bin/sh\ngit add -- hook.txt\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    commit::commit(&f.git, &f.request(vec![f.full("a.txt")])).unwrap();
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "previous staged");
    assert_eq!(f.git.text(["show", ":hook.txt"]).unwrap(), "hook addition");
    assert!(!f
        .git
        .text(["ls-tree", "--name-only", "HEAD"])
        .unwrap()
        .contains("hook.txt"));
    assert!(repository::status(&f.git).unwrap().recovery.is_empty());
}

#[test]
#[cfg(unix)]
fn conflicting_post_hook_staging_has_explicit_recovery_choice() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.initial();
    f.write("b.txt", "original staged\n");
    f.git.run(["add", "b.txt"]).unwrap();
    f.write("a.txt", "selected\n");
    let path = f.git.root.join(".git/hooks/post-commit");
    fs::write(
        &path,
        "#!/bin/sh\nprintf 'hook version\\n' > b.txt\ngit add -- b.txt\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let old_head = repository::head(&f.git).unwrap();
    let error = commit::commit(&f.git, &f.request(vec![f.full("a.txt")])).unwrap_err();
    assert!(error.contains("提交后钩子"), "{error}");
    assert_ne!(repository::head(&f.git).unwrap(), old_head);
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "original staged");
    let state = repository::status(&f.git).unwrap();
    assert_eq!(state.recovery.len(), 1);
    commit::recover(&f.git, &state.recovery[0], "hook").unwrap();
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "hook version");
    assert!(repository::status(&f.git).unwrap().recovery.is_empty());
}

#[test]
#[cfg(unix)]
fn cancellation_during_hook_keeps_head_and_index() {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "selected\n");
    hook(&f, "#!/bin/sh\ntouch .git/hook-started\nsleep 20\n");
    let started_file = f.git.root.join(".git/hook-started");
    let cancel = f.git.context.cancel.clone();
    let waiter = std::thread::spawn(move || {
        let started = Instant::now();
        while !started_file.exists() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(started_file.exists(), "钩子未启动");
        cancel.store(true, Ordering::SeqCst);
    });
    let head = repository::head(&f.git).unwrap();
    let index = repository::index_bytes(&f.git).unwrap();
    let outcome = commit::commit(&f.git, &f.request(vec![f.full("a.txt")]));
    waiter.join().unwrap();
    assert!(outcome.unwrap_err().contains("取消"));
    let git = Git::new(&f.git.root);
    assert_eq!(repository::head(&git).unwrap(), head);
    assert_eq!(repository::index_bytes(&git).unwrap(), index);
}

#[test]
fn interrupted_commit_recovers_index_without_overwriting_external_changes() {
    let f = Fixture::new();
    f.initial();
    f.write("a.txt", "selected\n");
    f.write("b.txt", "staged\n");
    f.git.run(["add", "b.txt"]).unwrap();
    let old_head = repository::head(&f.git).unwrap();
    let original = repository::index_bytes(&f.git).unwrap();
    commit::commit(&f.git, &f.request(vec![f.full("a.txt")])).unwrap();
    let result = repository::index_bytes(&f.git).unwrap();
    let new_head = repository::head(&f.git).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let directory = f.git.path("gitgui-recovery").unwrap().join(&id);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("original-index"), &original).unwrap();
    fs::write(directory.join("result-index"), &result).unwrap();
    fs::write(directory.join("transaction.json"), serde_json::to_vec(&serde_json::json!({
        "oldHead":old_head, "expectedTree":f.git.text(["rev-parse","HEAD^{tree}"]).unwrap(),
        "originalIndexHash":git::hash(&original), "resultIndexHash":git::hash(&result), "newHead":new_head,
        "pid":99999999, "postHookIndexHash":null, "postHookConflict":null
    })).unwrap()).unwrap();
    fs::write(f.git.path("index").unwrap(), &original).unwrap();
    f.write("extra.txt", "extra\n");
    f.git.run(["add", "extra.txt"]).unwrap();
    assert!(commit::recover(&f.git, &id, "original")
        .unwrap_err()
        .contains("修改"));
    fs::write(f.git.path("index").unwrap(), &original).unwrap();
    commit::recover(&f.git, &id, "original").unwrap();
    assert_eq!(f.git.text(["show", ":a.txt"]).unwrap(), "selected");
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "staged");
    assert!(repository::status(&f.git).unwrap().recovery.is_empty());
}

#[test]
fn amend_message_only_preserves_staged_and_unstaged_changes() {
    let f = Fixture::new();
    f.initial();
    f.write("b.txt", "staged remains\n");
    f.git.run(["add", "b.txt"]).unwrap();
    f.write("a.txt", "unstaged remains\n");
    let tree = f.git.text(["rev-parse", "HEAD^{tree}"]).unwrap();
    let mut request = f.request(vec![]);
    request.amend = true;
    request.message = "只修改提交说明".into();
    commit::commit(&f.git, &request).unwrap();
    assert_eq!(f.git.text(["rev-parse", "HEAD^{tree}"]).unwrap(), tree);
    assert_eq!(
        f.git.text(["log", "-1", "--format=%s"]).unwrap(),
        "只修改提交说明"
    );
    assert_eq!(f.git.text(["show", ":b.txt"]).unwrap(), "staged remains");
    assert_eq!(
        fs::read_to_string(f.git.root.join("a.txt")).unwrap(),
        "unstaged remains\n"
    );
}

#[test]
fn credential_helper_bridge_returns_only_answer_and_rejects_auth_failure() {
    use gitgui_lib::helper;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::time::Duration;
    let f = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::SeqCst) {
            if let Ok((mut stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 8192];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"GitGUI-Test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    });
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let recorded = prompts.clone();
    let ask: helper::Ask = Arc::new(move |prompt, _| {
        recorded.lock().unwrap().push(prompt.kind.clone());
        assert_eq!(prompt.kind, "askpass");
        Ok("synthetic-test-answer".into())
    });
    let bridge = helper::Bridge::start("credential-test".into(), &f.git.context, ask).unwrap();
    let mut git = f.git.clone();
    git.context.env.extend(bridge.env.clone());
    let response = git.raw([
        "-c",
        "http.proxy=",
        "-c",
        "credential.helper=",
        "ls-remote",
        &format!("http://127.0.0.1:{port}/repository.git"),
    ]);
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let response = response.unwrap();
    assert_ne!(response.code, 0);
    assert!(
        String::from_utf8_lossy(&response.stderr).contains("Authentication failed"),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    assert!(prompts.lock().unwrap().len() >= 2);
    assert!(!String::from_utf8_lossy(&response.stdout).contains("synthetic-test-answer"));
}

#[test]
fn editor_bridge_accepts_delayed_request_payload() {
    use gitgui_lib::helper;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::time::Duration;
    let f = Fixture::new();
    let ask: helper::Ask = Arc::new(|_, _| Ok("delayed-answer".into()));
    let bridge = helper::Bridge::start("delayed-connection".into(), &f.git.context, ask).unwrap();
    let env: std::collections::HashMap<_, _> = bridge
        .env
        .iter()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let mut stream = TcpStream::connect(&env["GITGUI_BRIDGE"]).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    // 让服务端先接受连接，再收到请求，稳定覆盖非阻塞 accept 的竞态。
    std::thread::sleep(Duration::from_millis(100));
    let message = serde_json::json!({"token":env["GITGUI_TOKEN"],"kind":"editor","content":"test"});
    writeln!(stream, "{message}").unwrap();
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).unwrap();
    let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["value"], "delayed-answer");
}

#[test]
fn independent_hunk_identity_survives_preceding_partial_commit() {
    let f = Fixture::new();
    let baseline = (0..80).map(|n| format!("line {n}\n")).collect::<String>();
    f.write("long.txt", &baseline);
    f.commit_all("baseline");
    let changed = baseline
        .replace("line 5\n", "line 5\nfirst insertion\n")
        .replace("line 65\n", "second change\n");
    f.write("long.txt", &changed);
    let id = repository::file_id(b"long.txt");
    let before = repository::diff(&f.git, &id, None, None, false).unwrap();
    assert_eq!(before.hunks.len(), 2);
    let remaining_id = before.hunks[1].id.clone();
    commit::commit(
        &f.git,
        &f.request(vec![f.partial("long.txt", "first insertion")]),
    )
    .unwrap();
    let after = repository::diff(&f.git, &id, None, None, false).unwrap();
    assert_eq!(after.hunks.len(), 1);
    assert_eq!(after.hunks[0].id, remaining_id);
}
