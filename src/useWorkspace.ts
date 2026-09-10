import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { api, desktop, errorMessage, subscribe } from './api';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { defaultGroups, pruneCommittedGroups } from './logic';
import type {
  CommitRequest,
  GitPrompt,
  OperationEvent,
  Preferences,
  Reference,
  Remote,
  RepoGroups,
  Repository,
  RepositoryStatus,
  Selection,
} from './types';

export interface Operation extends OperationEvent {
  logs: string;
}
const initialPrefs: Preferences = {
  recent: [],
  groups: {},
  theme: 'dark',
  layout: { sidebar: 290, history: 340 },
  drafts: {},
};

export function useWorkspace() {
  const [repo, setRepo] = useState<Repository | null>(null);
  const [status, setStatus] = useState<RepositoryStatus | null>(null);
  const [references, setReferences] = useState<Reference[]>([]);
  const [remotes, setRemotes] = useState<Remote[]>([]);
  const [preferences, setPreferences] = useState<Preferences>(initialPrefs);
  const [selections, setSelections] = useState<Record<string, Selection>>({});
  const [operations, setOperations] = useState<Operation[]>([]);
  const [prompts, setPrompts] = useState<GitPrompt[]>([]);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [createdPath, setCreatedPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [initialized, setInitialized] = useState(false);
  const [revision, setRevision] = useState(0);
  const latest = useRef({ repo, status, preferences, selections });
  latest.current = { repo, status, preferences, selections };
  const generation = useRef(0);
  const firstReady = useRef(true);
  const refreshGeneration = useRef(0);
  const refreshFn = useRef<() => Promise<void>>(async () => {});
  const openFn = useRef<(path: string) => Promise<void>>(async () => {});
  const commitOperations = useRef(new Map<string, CommitRequest>());
  const preferencesLoaded = useRef(false);
  const saveQueue = useRef(Promise.resolve());

  const updatePreferences = useCallback((update: (current: Preferences) => Preferences) => {
    setPreferences((current) => update(current));
  }, []);
  const updateGroups = useCallback(
    (update: (groups: RepoGroups) => RepoGroups) => {
      const repoId = latest.current.repo?.id;
      if (repoId)
        updatePreferences((p) => {
          const current = p.groups[repoId] ?? defaultGroups();
          const next = update(current);
          return next === current ? p : { ...p, groups: { ...p.groups, [repoId]: next } };
        });
    },
    [updatePreferences],
  );

  const refresh = useCallback(async () => {
    const current = latest.current.repo;
    if (!current) return;
    const seq = ++refreshGeneration.current;
    try {
      const [nextStatus, nextRefs, nextRemotes] = await Promise.all([
        api.status(current.id),
        api.refs(current.id),
        api.remotes(current.id),
      ]);
      if (seq !== refreshGeneration.current || latest.current.repo?.id !== current.id) return;
      if (latest.current.status && nextStatus.snapshot !== latest.current.status.snapshot) {
        setSelections({});
        if (Object.keys(latest.current.selections).length)
          setNotice('仓库已变化，请重新确认本次提交的选择。');
      }
      setStatus(nextStatus);
      setReferences(nextRefs);
      setRemotes(nextRemotes);
      setRevision((v) => v + 1);
    } catch (e) {
      if (latest.current.repo?.id === current.id) setError(errorMessage(e));
    }
  }, []);
  refreshFn.current = refresh;

  const openRepo = useCallback(
    async (path: string) => {
      const seq = ++generation.current;
      setLoading(true);
      setError('');
      setNotice('');
      try {
        const repositoryStartMs = performance.now();
        const next = await api.open(path);
        const repositoryOpenedMs = performance.now();
        const [nextStatus, nextRefs, nextRemotes] = await Promise.all([
          api.status(next.id),
          api.refs(next.id),
          api.remotes(next.id),
        ]);
        const queriesReadyMs = performance.now();
        if (seq !== generation.current) return;
        refreshGeneration.current++;
        setRepo(next);
        setStatus(nextStatus);
        setReferences(nextRefs);
        setRemotes(nextRemotes);
        setSelections({});
        setRevision((v) => v + 1);
        updatePreferences((p) => ({
          ...p,
          lastPath: next.path,
          recent: [next, ...p.recent.filter((r) => r.id !== next.id)].slice(0, 20),
        }));
        setLoading(false);
        if (desktop && firstReady.current) {
          firstReady.current = false;
          await getCurrentWindow().setFocus();
        }
        const beforePaintMs = performance.now();
        await new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        );
        if (seq === generation.current)
          await api.ready(next.id, {
            repositoryStartMs,
            repositoryOpenedMs,
            queriesReadyMs,
            beforePaintMs,
          });
      } catch (e) {
        if (seq === generation.current) setError(errorMessage(e));
      } finally {
        if (seq === generation.current) setLoading(false);
      }
    },
    [updatePreferences],
  );
  openFn.current = openRepo;

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    void subscribe({
      operation: (event) => {
        if (!alive) return;
        setOperations((current) => {
          const existing = current.find((o) => o.id === event.id);
          const operation = {
            ...event,
            logs: (
              (existing?.logs ?? '') +
              (event.state === 'progress' ? event.message : '\n' + event.message + '\n')
            ).slice(-64000),
          };
          if (existing) return current.map((o) => (o.id === event.id ? operation : o));
          return [operation, ...current].slice(0, 40);
        });
        if (['success', 'error', 'cancelled'].includes(event.state)) {
          setPrompts((current) => current.filter((prompt) => prompt.operationId !== event.id));
          const isCurrent = latest.current.repo?.id === event.repoId;
          if (isCurrent || event.repoId.startsWith('create:')) {
            if (event.state === 'error') setError(event.message);
            else setNotice(event.message);
          }
          if (event.state === 'success' && commitOperations.current.has(event.id)) {
            const repoId = event.repoId;
            const submitted = commitOperations.current.get(event.id)!;
            if (isCurrent) setSelections({});
            updatePreferences((p) => ({
              ...p,
              drafts: { ...p.drafts, [repoId]: '' },
              groups: {
                ...p.groups,
                [repoId]: pruneCommittedGroups(
                  p.groups[repoId] ?? defaultGroups(),
                  submitted.selections,
                ),
              },
            }));
          }
          commitOperations.current.delete(event.id);
          if (event.state === 'success' && event.repoId.startsWith('create:')) {
            const path = event.repoId.slice(7);
            setCreatedPath(path);
            void api
              .open(path)
              .then((repo) =>
                updatePreferences((p) => ({
                  ...p,
                  recent: [repo, ...p.recent.filter((r) => r.id !== repo.id)].slice(0, 20),
                })),
              )
              .catch((e) => setError(errorMessage(e)));
          } else if (isCurrent) void refreshFn.current();
        }
      },
      prompt: (prompt) => {
        if (alive) setPrompts((current) => [...current, prompt]);
      },
      changed: (repoId) => {
        if (alive && repoId === latest.current.repo?.id) void refreshFn.current();
      },
    })
      .then((fn) => {
        if (alive) unlisten = fn;
        else fn();
      })
      .catch((e) => setError(errorMessage(e)));
    if (desktop) {
      void api
        .preferences()
        .then(async (prefs) => {
          if (!alive) return;
          setPreferences(prefs);
          preferencesLoaded.current = true;
          setInitialized(true);
          if (prefs.lastPath) await openFn.current(prefs.lastPath);
          else await api.ready(null);
        })
        .catch((e) => {
          if (alive) {
            setError(errorMessage(e));
            setInitialized(true);
          }
        });
    } else {
      setInitialized(true);
    }
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [updatePreferences]);

  useEffect(() => {
    document.documentElement.dataset.theme = preferences.theme;
    if (!preferencesLoaded.current || !desktop) return;
    const timer = setTimeout(() => {
      saveQueue.current = saveQueue.current
        .catch(() => {})
        .then(() => api.savePreferences(preferences))
        .catch((e) => setError(errorMessage(e)));
    }, 250);
    return () => clearTimeout(timer);
  }, [preferences]);

  useEffect(() => {
    if (!desktop) return;
    const update = () => {
      void api
        .watch(
          document.visibilityState === 'visible' && document.hasFocus() ? (repo?.id ?? null) : null,
        )
        .catch((e) => setError(errorMessage(e)));
    };
    const focus = () => {
      update();
      void refreshFn.current();
    };
    update();
    window.addEventListener('focus', focus);
    window.addEventListener('blur', update);
    document.addEventListener('visibilitychange', update);
    return () => {
      window.removeEventListener('focus', focus);
      window.removeEventListener('blur', update);
      document.removeEventListener('visibilitychange', update);
    };
  }, [repo?.id]);

  const run = useCallback(async (kind: string, args: string[] = [], confirmed = false) => {
    const { repo, status } = latest.current;
    if (!repo || !status) throw new Error('请先打开仓库。');
    await api.action(repo.id, { kind, args, confirmed, snapshot: status.snapshot });
  }, []);
  const commitSelected = useCallback(async (message: string, amend: boolean, push: boolean) => {
    const { repo, status, selections } = latest.current;
    if (!repo || !status) throw new Error('请先打开仓库。');
    const request: CommitRequest = {
      message,
      amend,
      push,
      snapshot: status.snapshot,
      selections: Object.values(selections),
    };
    const id = await api.commit(repo.id, request);
    commitOperations.current.set(id, request);
  }, []);

  const busy = operations.some((o) => o.state === 'running' || o.state === 'progress');
  const groups = useMemo(
    () => (repo ? (preferences.groups[repo.id] ?? defaultGroups()) : defaultGroups()),
    [repo?.id, preferences.groups],
  );
  return {
    repo,
    status,
    references,
    remotes,
    preferences,
    updatePreferences,
    groups,
    updateGroups,
    selections,
    setSelections,
    operations,
    prompts,
    setPrompts,
    error,
    setError,
    notice,
    setNotice,
    createdPath,
    setCreatedPath,
    loading,
    initialized,
    revision,
    refresh,
    openRepo,
    run,
    commitSelected,
    busy,
  };
}
