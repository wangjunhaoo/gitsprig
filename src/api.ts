import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type {
  Repository,
  RepositoryStatus,
  FileDiff,
  Reference,
  Commit,
  HistoryQuery,
  FileChange,
  Remote,
  CommitRequest,
  GitAction,
  OperationEvent,
  GitPrompt,
  Preferences,
  ConflictFile,
  ShelfEntry,
  Worktree,
  AiSettings,
  SaveAiSettings,
  Selection,
  PushRequest,
} from './types';

export const desktop = isTauri();
const moduleReadyMs = performance.now();
const call = <T>(command: string, args: Record<string, unknown> = {}) => {
  if (!desktop) return Promise.reject<T>(new Error('请通过 GitSprig 桌面应用访问本地仓库。'));
  return invoke<T>(command, args);
};
export const api = {
  push: (repoId: string, request: PushRequest) => call<string>('start_push', { repoId, request }),
  queryPushRemote: (repoId: string, remote: string, requestId: string) =>
    call<string>('start_push_remote_query', { repoId, remote, requestId }),
  aiSettings: () => call<AiSettings>('load_ai_settings'),
  saveAiSettings: (request: SaveAiSettings) => call<AiSettings>('save_ai_settings', { request }),
  generateMessage: (repoId: string, requestId: string, snapshot: string, selections: Selection[]) =>
    call<string>('generate_commit_message', { repoId, requestId, snapshot, selections }),
  cancelAi: (requestId: string) => call<void>('cancel_ai_generation', { requestId }),
  open: (path: string) => call<Repository>('open_repository', { path }),
  status: (repoId: string) => call<RepositoryStatus>('repository_status', { repoId }),
  diff: (repoId: string, fileId: string, from?: string, to?: string, full = false) =>
    call<FileDiff>('file_diff', { repoId, fileId, from, to, full }),
  refs: (repoId: string) => call<Reference[]>('repository_refs', { repoId }),
  history: (repoId: string, query: HistoryQuery) =>
    call<Commit[]>('repository_history', { repoId, query }),
  commitFiles: (repoId: string, revision: string, base?: string) =>
    call<FileChange[]>('commit_files', { repoId, revision, base }),
  remotes: (repoId: string) => call<Remote[]>('repository_remotes', { repoId }),
  stash: (repoId: string) => call<ShelfEntry[]>('repository_collection', { repoId, kind: 'stash' }),
  reflog: (repoId: string) =>
    call<ShelfEntry[]>('repository_collection', { repoId, kind: 'reflog' }),
  worktrees: (repoId: string) =>
    call<Worktree[]>('repository_collection', { repoId, kind: 'worktree' }),
  blame: (repoId: string, fileId: string, revision?: string) =>
    call<string>('file_blame', { repoId, fileId, revision }),
  message: (repoId: string, revision: string) =>
    call<string>('commit_message', { repoId, revision }),
  template: (repoId: string) => call<string>('commit_template', { repoId }),
  commit: (repoId: string, request: CommitRequest) =>
    call<string>('start_commit', { repoId, request }),
  action: (repoId: string, action: GitAction) => call<string>('start_action', { repoId, action }),
  create: (path: string, url?: string) => call<string>('create_repository', { path, url }),
  cancel: (operationId: string) => call<void>('cancel_operation', { operationId }),
  answer: (promptId: string, value: string | null) =>
    call<void>('answer_prompt', { promptId, value }),
  conflict: (repoId: string, fileId: string) =>
    call<ConflictFile>('read_conflict', { repoId, fileId }),
  saveConflict: (repoId: string, fileId: string, expectedHash: string, content: string) =>
    call<string>('save_conflict', { repoId, fileId, expectedHash, content }),
  chooseConflict: (repoId: string, fileId: string, expectedHash: string, side: string) =>
    call<string>('choose_conflict', { repoId, fileId, expectedHash, side }),
  markConflict: (repoId: string, fileId: string, expectedHash: string) =>
    call<string>('mark_conflict', { repoId, fileId, expectedHash }),
  preferences: () => call<Preferences>('load_preferences'),
  savePreferences: (value: Preferences) => call<void>('save_preferences', { value }),
  watch: (repoId: string | null) => call<void>('watch_repository', { repoId }),
  ready: (repoId: string | null, phases: Record<string, number> = {}) =>
    call<void>('mark_ready', {
      repoId,
      timing: {
        moduleReadyMs,
        nowMs: performance.now(),
        visibility: document.visibilityState,
        focused: document.hasFocus(),
        ...phases,
      },
    }),
};

export async function subscribe(handlers: {
  operation: (event: OperationEvent) => void;
  prompt: (prompt: GitPrompt) => void;
  changed: (repoId: string) => void;
}): Promise<UnlistenFn> {
  if (!desktop) return () => {};
  const unlisten = await Promise.all([
    listen<OperationEvent>('git-operation', (e) => handlers.operation(e.payload)),
    listen<GitPrompt>('git-prompt', (e) => handlers.prompt(e.payload)),
    listen<string>('repository-changed', (e) => handlers.changed(e.payload)),
  ]);
  return () => unlisten.forEach((fn) => fn());
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
