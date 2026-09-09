use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: String,
    pub path: String,
    pub name: String,
    pub git_dir: String,
    pub common_dir: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub id: String,
    pub path: String,
    pub old_id: Option<String>,
    pub old_path: Option<String>,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub intent_to_add: bool,
    pub conflict: bool,
    pub submodule: bool,
    pub size: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStatus {
    pub snapshot: String,
    pub head: Option<String>,
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
    pub files: Vec<FileChange>,
    pub operation: Option<OperationState>,
    pub recovery: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OperationState {
    pub kind: String,
    pub detail: String,
    pub can_continue: bool,
    pub can_skip: bool,
    pub can_abort: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub id: String,
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub file_id: String,
    pub path: String,
    pub snapshot: String,
    pub content_hash: String,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    pub binary: bool,
    pub too_large: bool,
    pub size: u64,
    pub hunks: Vec<DiffHunk>,
    pub image_old: Option<String>,
    pub image_new: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub file_id: String,
    pub all: bool,
    pub line_ids: Vec<String>,
    pub content_hash: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitRequest {
    pub snapshot: String,
    pub selections: Vec<Selection>,
    pub message: String,
    pub amend: bool,
    pub push: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub oid: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub timestamp: i64,
    pub subject: String,
    pub decorations: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub revision: Option<String>,
    pub author: Option<String>,
    pub search: Option<String>,
    pub path: Option<String>,
    pub skip: usize,
    pub limit: usize,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub name: String,
    pub full_name: String,
    pub oid: String,
    pub current: bool,
    pub upstream: String,
    pub kind: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub file_id: String,
    pub path: String,
    pub snapshot: String,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub result: Option<String>,
    pub ours_label: String,
    pub theirs_label: String,
    pub binary: bool,
    pub ours_exists: bool,
    pub theirs_exists: bool,
    pub result_hash: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitAction {
    pub kind: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub confirmed: bool,
    pub snapshot: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OperationEvent {
    pub id: String,
    pub repo_id: String,
    pub state: String,
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub operation_id: String,
    pub kind: String,
    pub title: String,
    pub content: String,
}
