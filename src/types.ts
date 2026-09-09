export interface Repository {
  id: string;
  path: string;
  name: string;
  gitDir: string;
  commonDir: string;
}
export interface FileChange {
  id: string;
  path: string;
  oldId: string | null;
  oldPath: string | null;
  status: string;
  staged: boolean;
  unstaged: boolean;
  untracked: boolean;
  intentToAdd: boolean;
  conflict: boolean;
  submodule: boolean;
  size: number;
}
export interface OperationState {
  kind: string;
  detail: string;
  canContinue: boolean;
  canSkip: boolean;
  canAbort: boolean;
}
export interface RepositoryStatus {
  snapshot: string;
  head: string | null;
  branch: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: FileChange[];
  operation: OperationState | null;
  recovery: string[];
}
export interface DiffLine {
  id: string;
  kind: 'equal' | 'insert' | 'delete';
  text: string;
  oldLine: number | null;
  newLine: number | null;
}
export interface DiffHunk {
  id: string;
  oldStart: number;
  newStart: number;
  lines: DiffLine[];
}
export interface FileDiff {
  fileId: string;
  path: string;
  snapshot: string;
  contentHash: string;
  oldText: string | null;
  newText: string | null;
  binary: boolean;
  tooLarge: boolean;
  size: number;
  hunks: DiffHunk[];
  imageOld: string | null;
  imageNew: string | null;
}
export interface Selection {
  fileId: string;
  all: boolean;
  lineIds: string[];
  contentHash?: string | null;
}
export interface CommitRequest {
  snapshot: string;
  selections: Selection[];
  message: string;
  amend: boolean;
  push: boolean;
}
export interface Commit {
  oid: string;
  parents: string[];
  author: string;
  email: string;
  timestamp: number;
  subject: string;
  decorations: string;
}
export interface HistoryQuery {
  revision?: string;
  author?: string;
  search?: string;
  path?: string;
  skip: number;
  limit: number;
}
export interface Reference {
  name: string;
  fullName: string;
  oid: string;
  current: boolean;
  upstream: string;
  kind: 'local' | 'remote' | 'tag';
}
export interface Remote {
  name: string;
  fetchUrl: string;
  pushUrl: string;
}
export interface ConflictFile {
  fileId: string;
  path: string;
  snapshot: string;
  base: string | null;
  ours: string | null;
  theirs: string | null;
  result: string | null;
  oursLabel: string;
  theirsLabel: string;
  binary: boolean;
  oursExists: boolean;
  theirsExists: boolean;
  resultHash: string;
}
export interface GitAction {
  kind: string;
  args: string[];
  confirmed: boolean;
  snapshot?: string;
}
export interface OperationEvent {
  id: string;
  repoId: string;
  state: 'running' | 'progress' | 'success' | 'error' | 'cancelled';
  message: string;
}
export interface GitPrompt {
  id: string;
  operationId: string;
  kind: 'sequence' | 'editor' | 'askpass';
  title: string;
  content: string;
}
export interface ShelfEntry {
  name: string;
  oid: string;
  subject: string;
  timestamp: string;
}
export interface Worktree {
  worktree: string;
  HEAD: string;
  branch?: string;
  detached?: string;
  locked?: string;
  prunable?: string;
}
export interface ChangeGroup {
  id: string;
  name: string;
}
export interface HunkAssignment {
  groupId: string;
  contentHash: string;
  lineIds: string[];
  stale?: boolean;
}
export interface RepoGroups {
  groups: ChangeGroup[];
  fileGroups: Record<string, string>;
  hunkGroups: Record<string, Record<string, HunkAssignment>>;
}
export interface Preferences {
  recent: Repository[];
  groups: Record<string, RepoGroups>;
  theme: 'dark' | 'light';
  layout: { sidebar: number; history: number };
  drafts?: Record<string, string>;
  lastPath?: string;
}
export type View = 'changes' | 'history' | 'stash' | 'worktree' | 'reflog';
export type RunAction = (kind: string, args?: string[], confirmed?: boolean) => Promise<void>;
export interface AiSettings {
  endpoint: string;
  model: string;
  instruction: string;
  hasApiKey: boolean;
}

export interface SaveAiSettings {
  endpoint: string;
  model: string;
  instruction: string;
  apiKey: string | null;
  removeKey: boolean;
}
export interface PushRemoteInfo {
  remote: string;
  urls: string[];
  configuration: string;
  branches: { name: string; oid: string | null; destinations: number }[];
}

export interface PushRequest {
  remote: string;
  configuration: string;
  source: string;
  sourceOid: string;
  target: string;
  force: boolean;
  expected: string | null;
  setUpstream: boolean;
}
