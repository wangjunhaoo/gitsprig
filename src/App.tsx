import { lazy, Suspense, useCallback, useEffect, useRef, useState } from 'react';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import {
  Archive,
  ArrowDown,
  ArrowDownToLine,
  ArrowRight,
  ArrowUp,
  ArrowUpFromLine,
  Check,
  ChevronDown,
  CircleAlert,
  Clock3,
  Command,
  FileCode2,
  FolderGit2,
  FolderOpen,
  GitBranch,
  GitCommitHorizontal,
  GitCompareArrows,
  GitMerge,
  GitPullRequestArrow,
  Globe2,
  ListTree,
  LoaderCircle,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Settings2,
  Sparkles,
  Sun,
  TerminalSquare,
  Undo2,
  X,
} from 'lucide-react';
import { api, desktop, errorMessage } from './api';
import {
  basename,
  changedIds,
  fileStatus,
  shortOid,
  toggleLines,
  refreshGroupAnchors,
  setDiffLines,
} from './logic';
import { useWorkspace } from './useWorkspace';
import { useAiCommit } from './useAiCommit';
import type {
  ChangeGroup,
  Commit,
  FileChange,
  FileDiff,
  Reference,
  ShelfEntry,
  View,
} from './types';
import { BranchList, ChangesList } from './components/Sidebar';
import type { ChangeFileRow } from './components/Sidebar';
import {
  Dialog,
  Dropdown,
  Empty,
  IconButton,
  Menu,
  Modal,
  PromptDialog,
  ResizeHandle,
  Spinner,
} from './components/UI';
import type { DialogSpec, Field, MenuItem } from './components/UI';

const DiffEditor = lazy(() => import('./components/Editor'));
const MergeEditor = lazy(() => import('./components/MergeEditor'));
const History = lazy(() => import('./components/History'));
const Collections = lazy(() => import('./components/Collections'));
const Blame = lazy(() => import('./components/Blame'));
const AiSettingsDialog = lazy(() => import('./components/AiSettings'));
const CloneRepository = lazy(() => import('./components/CloneRepository'));
const PushDialog = lazy(() => import('./components/PushDialog'));

export default function App() {
  const ws = useWorkspace();
  const [view, setView] = useState<View>('changes');
  const [file, setFile] = useState<FileChange | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);
  const [historyFilter, setHistoryFilter] = useState('');
  const [historyPath, setHistoryPath] = useState('');
  const [dialog, setDialog] = useState<DialogSpec | null>(null);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [pushOpen, setPushOpen] = useState<{ force: boolean } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [logOpen, setLogOpen] = useState(false);
  const [remoteOpen, setRemoteOpen] = useState(false);
  const [amend, setAmend] = useState(false);
  const [mergeDirty, setMergeDirty] = useState(false);
  const [blame, setBlame] = useState<{ path: string; text: string } | null>(null);
  const [palette, setPalette] = useState(false);
  const [paletteSearch, setPaletteSearch] = useState('');
  const diffGeneration = useRef(0);
  const prevRepoId = useRef<string | null>(null);
  const currentView = useRef({ repoId: ws.repo?.id, fileId: file?.id });
  currentView.current = { repoId: ws.repo?.id, fileId: file?.id };
  const checkScope = useRef({
    repoId: ws.repo?.id,
    snapshot: ws.status?.snapshot,
    groups: ws.groups,
  });
  checkScope.current = { repoId: ws.repo?.id, snapshot: ws.status?.snapshot, groups: ws.groups };
  const closeGuard = useRef({ mergeDirty, busy: ws.busy });
  closeGuard.current = { mergeDirty, busy: ws.busy };
  const message = ws.repo ? (ws.preferences.drafts?.[ws.repo.id] ?? '') : '';
  const setMessage = (value: string) => {
    if (ws.repo)
      ws.updatePreferences((p) => ({ ...p, drafts: { ...p.drafts, [ws.repo!.id]: value } }));
  };
  const chosenCount = Object.keys(ws.selections).length;
  const pending = ws.operations.find((o) => o.state === 'running' || o.state === 'progress');
  const fail = (error: unknown) => ws.setError(errorMessage(error));
  const ai = useAiCommit({
    repoId: ws.repo?.id,
    snapshot: ws.status?.snapshot,
    selections: ws.selections,
    message,
    setMessage,
    notify: ws.setNotice,
    fail,
  });
  useEffect(() => {
    if (!desktop) return;
    let alive = true;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested((event) => {
        if (closeGuard.current.busy) {
          event.preventDefault();
          ws.setError('Git 操作仍在进行，请完成或取消操作后再关闭窗口。');
        } else if (closeGuard.current.mergeDirty) {
          event.preventDefault();
          setDialog({
            title: '放弃未保存的合并结果并退出？',
            description: '中间编辑区的改动尚未写入文件。',
            danger: true,
            confirmLabel: '放弃并退出',
            submit: async () => {
              setMergeDirty(false);
              await getCurrentWindow().destroy();
            },
          });
        }
      })
      .then((fn) => {
        if (alive) unlisten = fn;
        else fn();
      })
      .catch(fail);
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  const invokeAction = (kind: string, args: string[] = [], confirmed = false) =>
    void ws.run(kind, args, confirmed).catch(fail);

  const navigate = (fn: () => void) => {
    if (mergeDirty)
      setDialog({
        title: '离开未保存的合并结果？',
        description: '当前编辑尚未写入文件。离开后，这些编辑将丢失。',
        danger: true,
        confirmLabel: '放弃编辑并离开',
        submit: () => {
          setMergeDirty(false);
          fn();
        },
      });
    else fn();
  };
  const goView = (next: View) => navigate(() => setView(next));
  const goFile = (next: FileChange) => {
    if (next.id !== file?.id) navigate(() => setFile(next));
  };
  const pickRepo = async () => {
    if (!desktop) {
      ws.setError('请运行 GitSprig 桌面应用以访问本地仓库。');
      return;
    }
    const path = await openDialog({ directory: true, multiple: false, title: '打开 Git 仓库' });
    if (typeof path === 'string')
      navigate(() => {
        void ws.openRepo(path);
      });
  };
  const createRepo = async (clone: boolean) => {
    if (clone) {
      setCloneOpen(true);
      return;
    }
    const path = await saveDialog({
      title: '选择新仓库目录',
      defaultPath: 'my-repository',
    });
    if (!path) return;
    setDialog({
      title: '初始化仓库',
      description: `保存到 ${path}`,
      confirmLabel: '创建',
      submit: async () => {
        await api.create(path);
      },
    });
  };

  useEffect(() => {
    if (!ws.repo || !ws.status) return;
    if (prevRepoId.current !== ws.repo.id) {
      prevRepoId.current = ws.repo.id;
      setFile(ws.status.files[0] ?? null);
      setDiff(null);
      setAmend(false);
      setHistoryFilter('');
      setHistoryPath('');
      setView('changes');
      setMergeDirty(false);
      setBlame(null);
      if (desktop) void getCurrentWindow().setTitle(`${ws.repo.name} — GitSprig`).catch(fail);
    } else if (!mergeDirty)
      setFile(
        (current) =>
          ws.status!.files.find((f) => f.id === current?.id) ?? ws.status!.files[0] ?? null,
      );
  }, [ws.repo?.id, ws.status?.snapshot]);
  useEffect(() => {
    const generation = ++diffGeneration.current;
    if (!ws.repo || !file || file.conflict || view !== 'changes') {
      setDiff(null);
      setDiffLoading(false);
      return;
    }
    setDiffLoading(true);
    setDiff(null);
    void api
      .diff(ws.repo.id, file.id)
      .then((next) => {
        if (generation === diffGeneration.current) {
          setDiff(next);
          ws.updateGroups((groups) => refreshGroupAnchors(groups, next));
        }
      })
      .catch((error) => {
        if (generation === diffGeneration.current) fail(error);
      })
      .finally(() => {
        if (generation === diffGeneration.current) setDiffLoading(false);
      });
  }, [ws.repo?.id, file?.id, file?.conflict, ws.status?.snapshot, view]);

  const setFilesChecked = async (targets: ChangeFileRow[], include?: boolean) => {
    if (!ws.repo || !ws.status || !targets.length) return;
    const scope = checkScope.current;
    const requests = new Map<string, Set<string>>();
    for (const row of targets) {
      if (row.file.conflict) continue;
      const groups = requests.get(row.file.id) ?? new Set<string>();
      groups.add(row.groupId);
      requests.set(row.file.id, groups);
    }
    const updates: { fileId: string; detail?: FileDiff; ids: string[] }[] = [];
    for (const [fileId, groupIds] of requests) {
      const assignments = scope.groups.hunkGroups[fileId];
      if (assignments && Object.keys(assignments).length) {
        const detail =
          diff?.fileId === fileId && diff.snapshot === scope.snapshot
            ? diff
            : await api.diff(ws.repo.id, fileId);
        if (detail.snapshot !== scope.snapshot) throw new Error('文件已变化，请刷新后重新勾选。');
        const existing = new Set(detail.hunks.map((hunk) => hunk.id));
        if (Object.keys(assignments).some((id) => !existing.has(id))) {
          throw new Error('所选文件的代码块已变化，需要重新归组后勾选。');
        }
        const ids = changedIds(
          detail.hunks.filter((hunk) =>
            groupIds.has(
              assignments[hunk.id]?.groupId ?? scope.groups.fileGroups[fileId] ?? 'default',
            ),
          ),
        );
        if (!ids.length) throw new Error('所选分组中没有剩余代码块，请选择其他分组。');
        updates.push({ fileId, detail, ids });
      } else updates.push({ fileId, ids: [] });
    }
    if (
      checkScope.current.repoId !== scope.repoId ||
      checkScope.current.snapshot !== scope.snapshot ||
      (updates.some((item) => item.detail) && checkScope.current.groups !== scope.groups)
    ) {
      throw new Error('仓库或变更分组已变化，请重新勾选。');
    }
    ws.updateGroups((groups) =>
      updates.reduce(
        (current, update) =>
          update.detail ? refreshGroupAnchors(current, update.detail) : current,
        groups,
      ),
    );
    ws.setSelections((previous) => {
      const next = { ...previous };
      for (const update of updates) {
        const selection = update.detail
          ? include === undefined
            ? toggleLines(update.detail, next[update.fileId], update.ids)
            : setDiffLines(update.detail, next[update.fileId], update.ids, include)
          : (include ?? !next[update.fileId])
            ? { fileId: update.fileId, all: true, lineIds: [] }
            : undefined;
        if (selection) next[update.fileId] = selection;
        else delete next[update.fileId];
      }
      return next;
    });
  };
  const toggleDiffLines = (ids: string[]) => {
    if (!diff) return;
    ws.setSelections((previous) => {
      const next = { ...previous };
      const selection = toggleLines(diff, previous[diff.fileId], ids);
      if (selection) next[diff.fileId] = selection;
      else delete next[diff.fileId];
      return next;
    });
  };
  const moveFile = (target: FileChange, groupId: string) => {
    ws.updateGroups((current) => {
      const hunks = { ...current.hunkGroups };
      delete hunks[target.id];
      return {
        ...current,
        fileGroups: { ...current.fileGroups, [target.id]: groupId },
        hunkGroups: hunks,
      };
    });
  };
  const assignHunk = (hunkId: string, groupId: string) => {
    if (!diff) return;
    const hunk = diff.hunks.find((h) => h.id === hunkId);
    if (!hunk) return;
    ws.updateGroups((current) => ({
      ...current,
      hunkGroups: {
        ...current.hunkGroups,
        [diff.fileId]: {
          ...current.hunkGroups[diff.fileId],
          [hunkId]: { groupId, contentHash: diff.contentHash, lineIds: changedIds([hunk]) },
        },
      },
    }));
  };
  const newGroup = () =>
    setDialog({
      title: '新建变更组',
      description: '把同一任务的文件或代码块放在一起，单独选择和提交。',
      fields: [{ key: 'name', label: '名称', required: true, placeholder: '例如：修复登录问题' }],
      submit: (values) => {
        if (ws.groups.groups.some((g) => g.name === values.name))
          throw new Error('该变更组名称已存在。');
        ws.updateGroups((current) => ({
          ...current,
          groups: [...current.groups, { id: crypto.randomUUID(), name: values.name }],
        }));
      },
    });
  const groupMenu = (group: ChangeGroup): MenuItem[] => [
    {
      label: '重命名变更组…',
      action: () =>
        setDialog({
          title: '重命名变更组',
          fields: [{ key: 'name', label: '名称', value: group.name, required: true }],
          submit: (values) =>
            ws.updateGroups((current) => ({
              ...current,
              groups: current.groups.map((g) =>
                g.id === group.id ? { ...g, name: values.name } : g,
              ),
            })),
        }),
    },
    {
      label: '删除分组，改动移入默认组',
      disabled: group.id === 'default',
      action: () =>
        ws.updateGroups((current) => ({
          groups: current.groups.filter((g) => g.id !== group.id),
          fileGroups: Object.fromEntries(
            Object.entries(current.fileGroups).map(([id, value]) => [
              id,
              value === group.id ? 'default' : value,
            ]),
          ),
          hunkGroups: Object.fromEntries(
            Object.entries(current.hunkGroups).map(([fileId, hunks]) => [
              fileId,
              Object.fromEntries(
                Object.entries(hunks).map(([id, value]) => [
                  id,
                  value.groupId === group.id ? { ...value, groupId: 'default' } : value,
                ]),
              ),
            ]),
          ),
        })),
    },
  ];

  const branchFields = (base = 'HEAD'): Field[] => [
    { key: 'name', label: '新分支名称', placeholder: 'feature/my-change', required: true },
    { key: 'base', label: '起点', value: base, required: true },
  ];
  const createBranch = (base = 'HEAD') =>
    setDialog({
      title: '创建并切换分支',
      fields: branchFields(base),
      confirmLabel: '创建分支',
      submit: (values) => ws.run('branchCreate', [values.name, values.base]),
    });
  const confirmAction = (title: string, description: string, kind: string, args: string[]) =>
    setDialog({
      title,
      description,
      danger: true,
      confirmLabel: title,
      submit: () => ws.run(kind, args, true),
    });
  const newTag = (revision = 'HEAD') =>
    setDialog({
      title: '创建标签',
      fields: [
        { key: 'name', label: '标签名称', placeholder: 'v1.0.0', required: true },
        { key: 'revision', label: '提交', value: revision, required: true },
        { key: 'message', label: '说明（留空创建轻量标签）', type: 'textarea' },
      ],
      submit: (values) => ws.run('tagCreate', [values.name, values.revision, values.message]),
    });
  const rebase = (interactive: boolean, base = 'HEAD~1') =>
    setDialog({
      title: interactive ? '交互式变基' : '变基',
      description: '这会重写当前分支的提交。含合并提交时保留拓扑；工作区必须干净。',
      danger: true,
      fields: [
        { key: 'base', label: '基准提交或分支（全部历史填 --root）', value: base, required: true },
      ],
      confirmLabel: '开始变基',
      submit: (values) => ws.run(interactive ? 'interactiveRebase' : 'rebase', [values.base], true),
    });
  const reset = (revision: string) =>
    setDialog({
      title: '重置当前分支',
      description:
        '目标：' +
        revision +
        '。软重置保留暂存和工作区；混合重置取消暂存；硬重置会丢弃受影响的未提交改动。',
      danger: true,
      fields: [
        {
          key: 'mode',
          label: '重置方式',
          type: 'select',
          value: 'soft',
          options: [
            { value: 'soft', label: '软重置 · 保留全部改动' },
            { value: 'mixed', label: '混合重置 · 保留工作区' },
            { value: 'hard', label: '硬重置 · 丢弃受影响的改动' },
          ],
        },
      ],
      confirmLabel: '执行重置',
      submit: (values) => ws.run('reset', [values.mode, revision], true),
    });
  const branchMenu = (ref: Reference): MenuItem[] => [
    {
      label: ref.kind === 'remote' ? '创建跟踪分支并切换' : '切换到此分支',
      disabled: ref.current || ref.kind === 'tag',
      icon: <GitBranch size={14} />,
      action: () =>
        navigate(() => invokeAction(ref.kind === 'remote' ? 'track' : 'switch', [ref.name])),
    },
    {
      label: '从这里创建分支…',
      icon: <Plus size={14} />,
      action: () => createBranch(ref.fullName),
    },
    {
      label: '合并到当前分支',
      disabled: ref.current,
      icon: <GitMerge size={14} />,
      action: () =>
        setDialog({
          title: '合并分支',
          description: `将 ${ref.name} 合并到 ${ws.status?.branch}，遇到冲突时会暂停等待处理。`,
          confirmLabel: '合并',
          submit: () => ws.run('merge', [ref.fullName]),
        }),
    },
    {
      label: '将当前分支变基到这里…',
      disabled: ref.current,
      action: () => rebase(false, ref.fullName),
    },
    {
      label: '设置为当前分支上游',
      disabled: ref.kind !== 'remote',
      action: () => invokeAction('upstream', [ref.name]),
    },
    {
      label: '重命名…',
      disabled: ref.kind !== 'local',
      separator: true,
      action: () =>
        setDialog({
          title: '重命名分支',
          fields: [{ key: 'name', label: '新名称', value: ref.name, required: true }],
          submit: (values) => ws.run('branchRename', [ref.name, values.name]),
        }),
    },
    {
      label: ref.kind === 'tag' ? '删除标签…' : '删除已合并分支…',
      disabled: ref.current || ref.kind === 'remote',
      danger: true,
      action: () =>
        confirmAction(
          ref.kind === 'tag' ? '删除标签' : '删除分支',
          `删除本地 ${ref.name}。普通分支删除会检查是否已合并。`,
          ref.kind === 'tag' ? 'tagDelete' : 'branchDelete',
          [ref.name],
        ),
    },
    ...(ref.kind === 'local'
      ? [
          {
            label: '强制删除本地分支…',
            disabled: ref.current,
            danger: true,
            action: () =>
              confirmAction(
                '强制删除分支',
                `即使未合并，也删除本地分支 ${ref.name}。`,
                'branchDeleteForce',
                [ref.name],
              ),
          },
        ]
      : []),
  ];
  const applyCommit = (commit: Commit, kind: 'cherryPick' | 'revert') => {
    setDialog({
      title: kind === 'cherryPick' ? '挑选提交' : '撤销提交',
      description: `${shortOid(commit.oid)} · ${commit.subject}${kind === 'revert' ? '\n将创建一个反向提交，保留现有历史。' : '\n将此提交的改动应用到当前分支。'}`,
      fields:
        commit.parents.length > 1
          ? [
              {
                key: 'parent',
                label: '合并提交的主线父提交',
                type: 'select',
                value: '1',
                options: commit.parents.map((oid, index) => ({
                  value: String(index + 1),
                  label: `${index + 1} · ${shortOid(oid)}`,
                })),
              },
            ]
          : [],
      submit: (values) =>
        ws.run(kind, [commit.oid, ...(values.parent ? [values.parent] : [])], true),
    });
  };
  const commitMenu = (commit: Commit): MenuItem[] => [
    {
      label: '挑选此提交…',
      icon: <GitPullRequestArrow size={14} />,
      action: () => applyCommit(commit, 'cherryPick'),
    },
    {
      label: '撤销此提交…',
      icon: <Undo2 size={14} />,
      action: () => applyCommit(commit, 'revert'),
    },
    { label: '从这里创建分支…', separator: true, action: () => createBranch(commit.oid) },
    { label: '在这里创建标签…', action: () => newTag(commit.oid) },
    {
      label: '重置当前分支到这里…',
      danger: true,
      separator: true,
      action: () => reset(commit.oid),
    },
    { label: '从这里开始交互式变基…', action: () => rebase(true, commit.oid) },
    {
      label: '复制提交编号',
      action: () => {
        void navigator.clipboard.writeText(commit.oid).catch(fail);
      },
    },
  ];
  const showBlame = async (path: string, id?: string) => {
    if (!ws.repo) return;
    const repoId = ws.repo.id;
    const encoded =
      id ??
      btoa(String.fromCharCode(...new TextEncoder().encode(path)))
        .replace(/\+/g, '-')
        .replace(/\//g, '_')
        .replace(/=+$/, '');
    ws.setNotice('正在读取逐行历史…');
    try {
      const text = await api.blame(repoId, encoded);
      if (currentView.current.repoId === repoId) {
        setBlame({ path, text });
        ws.setNotice('');
      }
    } catch (e) {
      if (currentView.current.repoId === repoId) fail(e);
    }
  };
  const ignoreItems = (files: FileChange[]): MenuItem[] => {
    const eligible = files.filter(
      (item) => (item.untracked || item.intentToAdd) && !item.conflict && !item.submodule,
    );
    const names = [...new Set(eligible.map((item) => basename(item.path)))];
    const ignore = (byName: boolean) =>
      setDialog({
        title: byName ? '按文件名忽略' : `忽略 ${eligible.length} 个文件`,
        description:
          (byName
            ? '向仓库根目录 .gitignore 添加规则，忽略任意目录中的这些名称：\n\n' + names.join('\n')
            : '向仓库根目录 .gitignore 添加精确路径规则：\n\n' +
              eligible.map((item) => item.path).join('\n')) +
          '\n\n原文件保留在本地。已被 Git 跟踪的文件不受影响。' +
          (eligible.some((item) => item.intentToAdd)
            ? '\n将清除所选文件的待加入 Git 标记，其他暂存内容保持不变。'
            : ''),
        confirmLabel: '添加忽略规则',
        submit: () =>
          ws.run(
            byName ? 'ignoreNames' : 'ignorePaths',
            eligible.map((item) => item.id),
          ),
      });
    return [
      {
        label: eligible.length ? `忽略选中的 ${eligible.length} 个文件…` : '忽略（仅限未跟踪文件）',
        disabled: !eligible.length,
        action: () => ignore(false),
      },
      ...(eligible.length
        ? [
            {
              label:
                names.length === 1
                  ? `忽略所有位置的「${names[0]}」…`
                  : `按文件名忽略（${names.length} 种名称）…`,
              action: () => ignore(true),
            },
          ]
        : []),
    ];
  };
  const fileMenu = (target: FileChange, selected: FileChange[]): MenuItem[] =>
    selected.length > 1
      ? [
          ...ignoreItems(selected),
          ...ws.groups.groups.map((group, index) => ({
            label: `将所选 ${selected.length} 个文件移到「${group.name}」`,
            separator: index === 0,
            disabled: selected.some((item) => item.conflict),
            action: () => selected.forEach((item) => moveFile(item, group.id)),
          })),
        ]
      : [
          ...ignoreItems([target]),
          {
            label: '查看文件历史',
            icon: <Clock3 size={14} />,
            separator: true,
            action: () =>
              navigate(() => {
                setHistoryPath(target.path);
                setView('history');
              }),
          },
          {
            label: '逐行追溯…',
            action: () => {
              void showBlame(target.path, target.id);
            },
          },
          ...(target.submodule
            ? [
                {
                  label: '打开子模块仓库',
                  icon: <FolderOpen size={14} />,
                  action: () =>
                    navigate(() => {
                      void ws.openRepo(`${ws.repo!.path}/${target.path}`);
                    }),
                },
              ]
            : []),
          ...ws.groups.groups.map((group, index) => ({
            label: `移到「${group.name}」${Object.keys(ws.groups.hunkGroups[target.id] ?? {}).length ? '并重置代码块分组' : ''}`,
            separator: index === 0,
            action: () => moveFile(target, group.id),
          })),
          {
            label: '加入 Git 暂存区',
            separator: true,
            disabled: target.conflict,
            action: () => invokeAction('stage', [target.id]),
          },
          {
            label: '取消已有暂存',
            disabled: !target.staged || target.conflict,
            action: () => invokeAction('unstage', [target.id]),
          },
          {
            label: target.untracked ? '删除未跟踪文件…' : '丢弃这个文件的全部改动…',
            danger: true,
            separator: true,
            disabled: target.conflict || target.submodule,
            action: () =>
              confirmAction(
                target.untracked ? '删除文件' : '丢弃改动',
                `${target.path}\n${target.untracked ? '该文件尚未保存在 Git 中，删除后无法通过 Git 恢复。' : '工作区及暂存区中该文件的改动都将恢复到 HEAD。'}`,
                'discard',
                [target.id],
              ),
          },
        ];

  const stashCreate = () =>
    setDialog({
      title: '暂存当前工作',
      fields: [
        { key: 'message', label: '说明', placeholder: '例如：登录改动，待继续', required: true },
        { key: 'untracked', label: '包含未跟踪文件', type: 'checkbox', value: 'true' },
        { key: 'keep', label: '在工作区保留已暂存改动', type: 'checkbox', value: 'false' },
      ],
      confirmLabel: '保存到 Stash',
      submit: (values) => ws.run('stashCreate', [values.message, values.untracked, values.keep]),
    });
  const worktreeCreate = async () => {
    const path = await saveDialog({ title: '选择新工作树目录', defaultPath: 'parallel-worktree' });
    if (!path) return;
    setDialog({
      title: '添加工作树',
      description: path,
      fields: [
        { key: 'branch', label: '分支名称', required: true },
        {
          key: 'mode',
          label: '分支方式',
          type: 'select',
          value: 'new',
          options: [
            { value: 'new', label: '创建新分支' },
            { value: 'existing', label: '使用已有本地分支' },
          ],
        },
        { key: 'base', label: '新分支起点', value: 'HEAD' },
      ],
      submit: (values) => ws.run('worktreeAdd', [path, values.branch, values.mode, values.base]),
    });
  };
  const shelfMenu = (entry: ShelfEntry): MenuItem[] =>
    view === 'stash'
      ? [
          {
            label: '应用，保留 Stash…',
            action: () =>
              setDialog({
                title: '应用暂存工作',
                description: entry.subject,
                fields: [{ key: 'index', label: '同时恢复原暂存区', type: 'checkbox' }],
                submit: (values) => ws.run('stashApply', [entry.oid, values.index]),
              }),
          },
          {
            label: '应用并移除 Stash…',
            action: () =>
              setDialog({
                title: '应用并移除暂存工作',
                description: '仅在成功应用后移除记录；冲突时保留 Stash。',
                fields: [{ key: 'index', label: '同时恢复原暂存区', type: 'checkbox' }],
                submit: (values) => ws.run('stashPop', [entry.name, values.index]),
              }),
          },
          {
            label: '删除 Stash…',
            danger: true,
            separator: true,
            action: () =>
              confirmAction('删除暂存记录', entry.name + ' · ' + entry.subject, 'stashDrop', [
                entry.name,
              ]),
          },
        ]
      : [
          {
            label: '从此记录恢复为新分支…',
            action: () =>
              setDialog({
                title: '恢复提交',
                description: '创建新分支指向这个提交，当前工作区保持原状。',
                fields: [
                  {
                    key: 'name',
                    label: '恢复分支名称',
                    required: true,
                    value: 'recovered-' + shortOid(entry.oid),
                  },
                ],
                submit: (values) => ws.run('reflogRestore', [entry.oid, values.name]),
              }),
          },
          { label: '重置当前分支到这里…', danger: true, action: () => reset(entry.oid) },
        ];
  const remoteEdit = (name?: string, url?: string) => {
    setRemoteOpen(false);
    setDialog({
      title: name ? '修改远程地址' : '添加远程仓库',
      fields: [
        { key: 'name', label: '名称', value: name ?? 'origin', required: true },
        { key: 'url', label: '仓库地址', value: url ?? '', required: true },
      ],
      submit: (values) => ws.run(name ? 'remoteSetUrl' : 'remoteAdd', [values.name, values.url]),
    });
  };
  const pushDialog = (force = false) => {
    if (!ws.status) return;
    if (!ws.remotes.length) {
      setRemoteOpen(true);
      return;
    }
    setPushOpen({ force });
  };
  const commitNow = async (push: boolean) => {
    if (amend) {
      setDialog({
        title: '修改上次提交',
        description: '上次提交的编号将改变。如果已经推送，请先与协作者确认。',
        danger: true,
        confirmLabel: '修改提交',
        submit: () => ws.commitSelected(message, true, push),
      });
    } else {
      try {
        await ws.commitSelected(message, false, push);
      } catch (e) {
        fail(e);
      }
    }
  };
  const amendToggle = async (checked: boolean) => {
    setAmend(checked);
    if (checked && !message && ws.repo && ws.status?.head) {
      try {
        setMessage(await api.message(ws.repo.id, ws.status.head));
      } catch (e) {
        fail(e);
      }
    }
  };
  const template = async () => {
    if (ws.repo) {
      try {
        const text = await api.template(ws.repo.id);
        if (text) setMessage(text);
        else ws.setNotice('此仓库没有配置提交模板。');
      } catch (e) {
        fail(e);
      }
    }
  };
  const allCommands: MenuItem[] = [
    {
      label: 'AI 提交说明设置…',
      icon: <Sparkles size={14} />,
      action: () => ai.setSettingsOpen(true),
    },
    {
      label: '打开仓库…',
      icon: <FolderOpen size={14} />,
      shortcut: '⌘ O',
      action: () => {
        void pickRepo().catch(fail);
      },
    },
    {
      label: '创建分支…',
      disabled: !ws.repo,
      icon: <GitBranch size={14} />,
      action: () => createBranch(),
    },
    {
      label: '暂存当前工作…',
      disabled: !ws.repo,
      icon: <Archive size={14} />,
      action: stashCreate,
    },
    {
      label: '交互式变基…',
      disabled: !ws.status?.head,
      icon: <GitCompareArrows size={14} />,
      action: () => rebase(true),
    },
    { label: '创建标签…', disabled: !ws.status?.head, action: () => newTag() },
    {
      label: '添加工作树…',
      disabled: !ws.repo,
      action: () => {
        void worktreeCreate().catch(fail);
      },
    },
    {
      label: '管理远程仓库…',
      disabled: !ws.repo,
      icon: <Globe2 size={14} />,
      action: () => setRemoteOpen(true),
    },
    {
      label: '安全强制推送…',
      disabled: !ws.repo || !ws.remotes.length,
      action: () => pushDialog(true),
    },
    {
      label: '查看文件历史…',
      disabled: !ws.repo,
      action: () =>
        setDialog({
          title: '文件历史',
          fields: [{ key: 'path', label: '仓库内相对路径', required: true }],
          submit: (values) =>
            navigate(() => {
              setHistoryPath(values.path);
              setView('history');
            }),
        }),
    },
    {
      label: '逐行追溯文件…',
      disabled: !ws.repo,
      action: () =>
        setDialog({
          title: '逐行追溯',
          fields: [{ key: 'path', label: '仓库内相对路径', required: true }],
          submit: (values) => showBlame(values.path),
        }),
    },
    {
      label: '刷新仓库',
      disabled: !ws.repo,
      shortcut: '⌘ R',
      icon: <RefreshCw size={14} />,
      action: () => {
        void ws.refresh();
      },
    },
    {
      label: ws.preferences.theme === 'dark' ? '切换浅色主题' : '切换深色主题',
      icon: <Sun size={14} />,
      action: () =>
        ws.updatePreferences((p) => ({ ...p, theme: p.theme === 'dark' ? 'light' : 'dark' })),
    },
  ];
  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => {});
  const nativeHandler = useRef<(id: string) => void>(() => {});
  nativeHandler.current = (id) => {
    if (id === 'quit' || id === 'close') {
      void getCurrentWindow().close().catch(fail);
      return;
    }
    if (dialog || ws.prompts.length) return;
    switch (id) {
      case 'open':
        void pickRepo().catch(fail);
        break;
      case 'clone':
        void createRepo(true).catch(fail);
        break;
      case 'init':
        void createRepo(false).catch(fail);
        break;
      case 'changes':
        goView('changes');
        break;
      case 'history':
        goView('history');
        break;
      case 'refresh':
        void ws.refresh();
        break;
      case 'palette':
        setPalette(true);
        break;
      case 'push':
        if (ws.repo && !ws.busy) pushDialog();
        break;
      case 'pull':
        if (ws.repo && !ws.busy) invokeAction('pull');
        break;
      case 'fetch':
        if (ws.repo && !ws.busy) invokeAction('fetch');
        break;
      case 'branch':
        if (ws.repo) createBranch();
        break;
      case 'stash':
        if (ws.repo) stashCreate();
        break;
      case 'rebase':
        if (ws.status?.head) rebase(true);
        break;
      case 'commit-focus':
        navigate(() => {
          setView('changes');
          requestAnimationFrame(() => document.getElementById('commit-message')?.focus());
        });
        break;
      case 'commit-now':
        if ((chosenCount || amend) && message.trim() && !ws.busy && !ws.status?.operation)
          void commitNow(false);
        break;
    }
  };
  useEffect(() => {
    if (!desktop) return;
    let alive = true;
    let dispose: (() => void) | undefined;
    void listen<string>('native-menu', (event) => nativeHandler.current(event.payload))
      .then((fn) => {
        if (alive) dispose = fn;
        else fn();
      })
      .catch(fail);
    return () => {
      alive = false;
      dispose?.();
    };
  }, []);
  keyHandler.current = (event) => {
    if (desktop) return;
    if (!(event.metaKey || event.ctrlKey) || dialog || ws.prompts.length) return;
    if (event.key.toLowerCase() === 'o') {
      event.preventDefault();
      void pickRepo().catch(fail);
    } else if (
      event.key === 'Enter' &&
      view === 'changes' &&
      (chosenCount || amend) &&
      message.trim() &&
      !ws.busy
    ) {
      event.preventDefault();
      void commitNow(false);
    } else if (event.key.toLowerCase() === 'k') {
      event.preventDefault();
      if (event.shiftKey) pushDialog();
      else {
        goView('changes');
        setTimeout(() => document.getElementById('commit-message')?.focus(), 0);
      }
    } else if (event.key === '9') {
      event.preventDefault();
      goView('history');
    } else if (event.key === '0') {
      event.preventDefault();
      goView('changes');
    } else if (event.key.toLowerCase() === 'r') {
      event.preventDefault();
      void ws.refresh();
    } else if (event.shiftKey && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      setPalette(true);
    }
  };
  useEffect(() => {
    const handler = (event: KeyboardEvent) => keyHandler.current(event);
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);

  const viewNames = {
    changes: '提交',
    history: '提交历史',
    stash: '暂存工作',
    worktree: '工作树',
    reflog: '操作历史',
  };
  return (
    <div className="app-shell">
      <header className="titlebar">
        <div className="titlebar-drag" data-tauri-drag-region />
        <div className="app-wordmark" data-tauri-drag-region>
          <GitBranch size={17} />
          <span>GitSprig</span>
        </div>
        <div className="titlebar-divider" />
        <Dropdown
          label={ws.repo?.name ?? '打开仓库'}
          icon={<FolderGit2 size={15} />}
          items={[
            {
              label: '打开本地仓库…',
              icon: <FolderOpen size={14} />,
              shortcut: '⌘ O',
              action: () => {
                void pickRepo().catch(fail);
              },
            },
            {
              label: '克隆仓库…',
              action: () => {
                void createRepo(true).catch(fail);
              },
            },
            {
              label: '初始化仓库…',
              action: () => {
                void createRepo(false).catch(fail);
              },
            },
            ...ws.preferences.recent.map((repo, i) => ({
              label: repo.name,
              separator: i === 0,
              action: () =>
                navigate(() => {
                  void ws.openRepo(repo.path);
                }),
            })),
          ]}
        />
        {ws.status && (
          <>
            <span className="titlebar-divider" />
            <Dropdown
              label={ws.status.branch === '(detached)' ? '游离 HEAD' : ws.status.branch}
              icon={<GitBranch size={14} />}
              items={[
                { label: '创建分支…', icon: <Plus size={14} />, action: () => createBranch() },
                ...ws.references
                  .filter((r) => r.kind === 'local')
                  .slice(0, 25)
                  .map((ref, i) => ({
                    label: ref.name,
                    icon: ref.current ? <Check size={14} /> : <GitBranch size={14} />,
                    disabled: ref.current,
                    separator: i === 0,
                    action: () => navigate(() => invokeAction('switch', [ref.name])),
                  })),
              ]}
            />
          </>
        )}
        <span className="flex-spacer" data-tauri-drag-region />
        {ws.repo && (
          <div className="git-toolbar">
            <IconButton
              title="获取远程更新"
              disabled={ws.busy}
              onClick={() => invokeAction('fetch')}
            >
              <RefreshCw size={15} />
            </IconButton>
            <button
              className="toolbar-button"
              disabled={ws.busy}
              onClick={() => invokeAction('pull')}
            >
              <ArrowDownToLine size={15} />
              <span>拉取</span>
              {!!ws.status?.behind && <small>{ws.status.behind}</small>}
            </button>
            <button className="toolbar-button" disabled={ws.busy} onClick={() => pushDialog()}>
              <ArrowUpFromLine size={15} />
              <span>推送</span>
              {!!ws.status?.ahead && <small>{ws.status.ahead}</small>}
            </button>
            <IconButton
              title="更多 Git 操作"
              onClick={() => setMenu({ x: window.innerWidth - 285, y: 44, items: allCommands })}
            >
              <span>
                <MoreHorizontal size={18} />
              </span>
            </IconButton>
          </div>
        )}
        <IconButton title="查找操作 ⇧⌘A" onClick={() => setPalette(true)}>
          <Search size={16} />
        </IconButton>
      </header>

      {!ws.initialized || ws.loading ? (
        <div className="startup">
          <Spinner text="正在打开仓库…" />
        </div>
      ) : !ws.repo || !ws.status ? (
        <main className="welcome">
          <div className="welcome-content">
            <div className="welcome-logo">
              <GitBranch size={39} />
            </div>
            <h1>打开你的仓库</h1>
            <p>专注改动、审阅与提交。</p>
            <div className="welcome-actions">
              <button className="button primary" onClick={() => void pickRepo().catch(fail)}>
                <FolderOpen size={16} />
                打开本地仓库<kbd>⌘ O</kbd>
              </button>
              <button className="button" onClick={() => void createRepo(true).catch(fail)}>
                <ArrowDownToLine size={16} />
                克隆仓库
              </button>
            </div>
            <button className="text-button" onClick={() => void createRepo(false).catch(fail)}>
              <Plus size={13} />
              初始化新仓库
            </button>
            {ws.preferences.recent.length > 0 && (
              <div className="recent-repositories">
                <div className="section-eyebrow">最近打开</div>
                {ws.preferences.recent.slice(0, 6).map((repo) => (
                  <button key={repo.id} onClick={() => void ws.openRepo(repo.path)}>
                    <FolderGit2 size={18} />
                    <span>
                      <strong>{repo.name}</strong>
                      <small>{repo.path}</small>
                    </span>
                    <ArrowRight size={15} />
                  </button>
                ))}
              </div>
            )}
          </div>
          <div className="welcome-bottom">
            <span>
              <kbd>⌘ K</kbd> 准备提交
            </span>
            <span>
              <kbd>⌘ 9</kbd> 查看历史
            </span>
            <span>
              <kbd>⇧ ⌘ A</kbd> 查找操作
            </span>
          </div>
        </main>
      ) : (
        <div className="workspace">
          <nav className="activity-bar">
            {(
              [
                ['changes', ListTree, '⌘0'],
                ['history', GitCommitHorizontal, '⌘9'],
                ['stash', Archive, ''],
                ['worktree', FolderGit2, ''],
                ['reflog', Clock3, ''],
              ] as const
            ).map(([key, Icon, shortcut]) => (
              <IconButton
                key={key}
                title={viewNames[key] + (shortcut ? ' ' + shortcut : '')}
                active={view === key}
                onClick={() => goView(key)}
              >
                <Icon size={20} />
                {key === 'changes' && !!ws.status?.files.length && <i className="activity-dot" />}
              </IconButton>
            ))}
            <span className="flex-spacer" />
            <IconButton title="操作输出" active={logOpen} onClick={() => setLogOpen((v) => !v)}>
              <TerminalSquare size={19} />
            </IconButton>
            <IconButton
              title="外观与偏好"
              onClick={() =>
                setDialog({
                  title: '外观与偏好',
                  description:
                    '轻枝 GitSprig 使用本机 Git 及现有凭据配置。时间按 Asia/Shanghai 显示。',
                  fields: [
                    {
                      key: 'theme',
                      label: '主题',
                      type: 'select',
                      value: ws.preferences.theme,
                      options: [
                        { value: 'dark', label: '深色' },
                        { value: 'light', label: '浅色' },
                      ],
                    },
                  ],
                  submit: (values) =>
                    ws.updatePreferences((p) => ({
                      ...p,
                      theme: values.theme as 'dark' | 'light',
                    })),
                })
              }
            >
              <Settings2 size={18} />
            </IconButton>
          </nav>
          <aside className="sidebar" style={{ width: ws.preferences.layout.sidebar }}>
            {view === 'changes' ? (
              <>
                <ChangesList
                  key={ws.repo.id}
                  status={ws.status}
                  selectedFile={file?.id ?? null}
                  selections={ws.selections}
                  groups={ws.groups}
                  onFile={goFile}
                  onSetChecked={(rows, include) => setFilesChecked(rows, include).catch(fail)}
                  onNewGroup={newGroup}
                  fileMenu={fileMenu}
                  groupMenu={groupMenu}
                />
                <div className="selection-summary">
                  <button
                    className="text-button"
                    onClick={() =>
                      ws.setSelections(
                        Object.fromEntries(
                          ws
                            .status!.files.filter((f) => !f.conflict)
                            .map((f) => [f.id, { fileId: f.id, all: true, lineIds: [] }]),
                        ),
                      )
                    }
                  >
                    全部勾选
                  </button>
                  <button className="text-button" onClick={() => ws.setSelections({})}>
                    取消勾选
                  </button>
                  <span className="flex-spacer" />
                  <span>待提交 {chosenCount} 个文件</span>
                </div>
                <div className="commit-composer">
                  <div className="composer-heading">
                    <span>提交说明</span>
                    <span className="flex-spacer" />
                    {ai.busy ? (
                      <button className="text-button ai-generate" onClick={() => void ai.cancel()}>
                        <LoaderCircle size={12} className="spin" />
                        取消生成
                      </button>
                    ) : (
                      <button
                        className="text-button ai-generate"
                        title="根据本次勾选的改动生成提交说明"
                        disabled={!chosenCount || ws.busy || !!ws.status.operation}
                        onClick={() => void ai.generate()}
                      >
                        <Sparkles size={12} />
                        AI 生成
                      </button>
                    )}
                    <IconButton title="AI 提交说明设置" onClick={() => ai.setSettingsOpen(true)}>
                      <Settings2 size={12} />
                    </IconButton>
                    <button className="text-button" onClick={() => void template()}>
                      模板
                    </button>
                  </div>
                  <textarea
                    id="commit-message"
                    placeholder="描述这次改动…"
                    value={message}
                    onChange={(event) => setMessage(event.target.value)}
                    spellCheck={false}
                  />
                  <label className="amend-option">
                    <input
                      type="checkbox"
                      checked={amend}
                      disabled={!ws.status.head || !!ws.status.operation}
                      onChange={(event) => void amendToggle(event.target.checked)}
                    />
                    修改上次提交 <span>Amend</span>
                  </label>
                  <div className="commit-buttons">
                    <button
                      className="button primary commit-button"
                      disabled={
                        (!chosenCount && !amend) ||
                        !message.trim() ||
                        ws.busy ||
                        !!ws.status.operation
                      }
                      onClick={() => void commitNow(false)}
                    >
                      <GitCommitHorizontal size={16} />
                      提交<kbd>⌘ ↵</kbd>
                    </button>
                    <button
                      className="button primary commit-more"
                      title="提交并推送"
                      disabled={
                        (!chosenCount && !amend) ||
                        !message.trim() ||
                        ws.busy ||
                        !!ws.status.operation
                      }
                      onClick={() =>
                        setDialog({
                          title: '提交并推送',
                          description: '提交选中的改动后，推送到当前分支已配置的上游。',
                          confirmLabel: '提交并推送',
                          submit: () => ws.commitSelected(message, amend, true),
                        })
                      }
                    >
                      <ArrowUp size={16} />
                    </button>
                  </div>
                </div>
              </>
            ) : (
              <BranchList
                references={ws.references}
                selected={historyFilter}
                onSelect={(ref) =>
                  navigate(() => {
                    setHistoryFilter(ref);
                    setView('history');
                  })
                }
                menuFor={branchMenu}
                onNew={() => createBranch()}
              />
            )}
            <div className="sidebar-bottom">
              <FolderGit2 size={13} />
              <span title={ws.repo.path}>{ws.repo.name}</span>
              <span className="flex-spacer" />
              <small>本地仓库</small>
            </div>
          </aside>
          <ResizeHandle
            value={ws.preferences.layout.sidebar}
            onChange={(sidebar) =>
              ws.updatePreferences((p) => ({ ...p, layout: { ...p.layout, sidebar } }))
            }
          />
          <main className="main-panel">
            <div className="workspace-tabs">
              <button
                className={view === 'changes' ? 'selected' : ''}
                onClick={() => goView('changes')}
              >
                <ListTree size={14} />
                工作区{ws.status.files.length > 0 && <span>{ws.status.files.length}</span>}
              </button>
              <button
                className={view === 'history' ? 'selected' : ''}
                onClick={() => goView('history')}
              >
                <GitCommitHorizontal size={14} />
                提交历史
              </button>
              {!['changes', 'history'].includes(view) && (
                <button className="selected">{viewNames[view]}</button>
              )}
              <span className="flex-spacer" />
              <IconButton title="刷新 ⌘R" onClick={() => void ws.refresh()}>
                <RefreshCw size={14} />
              </IconButton>
            </div>
            {ws.status.operation && (
              <div className="operation-banner">
                <GitMerge size={16} />
                <span>
                  <strong>{ws.status.operation.kind}</strong> · {ws.status.operation.detail}
                </span>
                <span className="flex-spacer" />
                {ws.status.operation.kind === 'rebase' && (
                  <>
                    <button
                      className="text-button"
                      disabled={ws.busy}
                      onClick={() => invokeAction('editTodo')}
                    >
                      编辑剩余序列
                    </button>
                    <button
                      className="text-button"
                      onClick={() =>
                        setDialog({
                          title: '修改暂停处的提交',
                          description: '仅纳入已暂存的改动；可以先通过文件右键菜单加入暂存区。',
                          fields: [
                            { key: 'message', label: '提交说明', type: 'textarea', required: true },
                          ],
                          submit: (values) => ws.run('amendPaused', [values.message]),
                        })
                      }
                    >
                      修改当前提交
                    </button>
                  </>
                )}
                <button
                  className="button small-button"
                  disabled={!ws.status.operation.canContinue || ws.busy || mergeDirty}
                  onClick={() => invokeAction('continue')}
                >
                  继续
                </button>
                {ws.status.operation.canSkip && (
                  <button
                    className="text-button"
                    disabled={ws.busy}
                    onClick={() =>
                      confirmAction('跳过当前提交', '当前正在应用的提交将被跳过。', 'skip', [])
                    }
                  >
                    跳过
                  </button>
                )}
                <button
                  className="text-button danger-text"
                  disabled={ws.busy}
                  onClick={() =>
                    confirmAction(
                      '终止当前操作',
                      '恢复到操作开始前的分支状态；本次冲突处理的未提交改动可能被丢弃。',
                      'abort',
                      [],
                    )
                  }
                >
                  终止
                </button>
              </div>
            )}
            {ws.status.recovery.map((id) => (
              <div className="recovery-banner" key={id}>
                <CircleAlert size={16} />
                <span>发现中断的提交，需要检查暂存区恢复。</span>
                <button
                  className="button small-button"
                  onClick={() =>
                    setDialog({
                      title: '恢复提交状态',
                      description:
                        '仅在当前提交和索引仍与恢复记录一致时执行。若后置钩子与原暂存内容冲突，请选择要保留的一份状态。',
                      danger: true,
                      fields: [
                        {
                          key: 'choice',
                          label: '保留方式',
                          type: 'select',
                          value: 'original',
                          options: [
                            { value: 'original', label: '保留原来的剩余暂存内容' },
                            { value: 'hook', label: '采用后置钩子的暂存状态（若有）' },
                          ],
                        },
                      ],
                      submit: (values) => ws.run('recoverIndex', [id, values.choice], true),
                    })
                  }
                >
                  检查并恢复
                </button>
              </div>
            ))}
            <div className="main-content">
              <Suspense fallback={<Spinner />}>
                {view === 'changes' ? (
                  file ? (
                    <>
                      <div className="file-tab">
                        <FileCode2 size={14} className={`file-${fileStatus(file.status)}`} />
                        <span>{basename(file.path)}</span>
                        <span className={`file-status file-${fileStatus(file.status)}`}>
                          {fileStatus(file.status)}
                        </span>
                        <span className="flex-spacer" />
                        <span className="muted">
                          {file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}
                        </span>
                        {file.submodule && (
                          <button
                            className="text-button"
                            onClick={() =>
                              navigate(() => {
                                void ws.openRepo(`${ws.repo!.path}/${file.path}`);
                              })
                            }
                          >
                            打开子模块
                          </button>
                        )}
                      </div>
                      {file.conflict ? (
                        <MergeEditor
                          repoId={ws.repo.id}
                          fileId={file.id}
                          onDirtyChange={setMergeDirty}
                          onResolved={() => {
                            setMergeDirty(false);
                            void ws.refresh();
                          }}
                          onError={ws.setError}
                        />
                      ) : diffLoading ? (
                        <Spinner text="正在读取差异…" />
                      ) : diff ? (
                        <DiffEditor
                          diff={diff}
                          selection={ws.selections[file.id]}
                          onToggle={toggleDiffLines}
                          onFull={() => {
                            const generation = ++diffGeneration.current;
                            setDiffLoading(true);
                            void api
                              .diff(ws.repo!.id, file.id, undefined, undefined, true)
                              .then((value) => {
                                if (generation === diffGeneration.current) setDiff(value);
                              })
                              .catch((error) => {
                                if (generation === diffGeneration.current) fail(error);
                              })
                              .finally(() => {
                                if (generation === diffGeneration.current) setDiffLoading(false);
                              });
                          }}
                          groups={ws.groups.groups}
                          hunkGroups={ws.groups.hunkGroups[file.id]}
                          onGroupHunk={assignHunk}
                        />
                      ) : (
                        <Empty title="没有可显示的差异" />
                      )}
                    </>
                  ) : (
                    <Empty
                      icon={<GitCommitHorizontal size={34} />}
                      title="工作区干净"
                      detail="修改文件后，改动会自动出现在这里。"
                    />
                  )
                ) : view === 'history' ? (
                  <History
                    repoId={ws.repo.id}
                    references={ws.references}
                    revisionFilter={historyFilter}
                    onRevisionFilter={setHistoryFilter}
                    pathFilter={historyPath}
                    refresh={ws.revision}
                    height={ws.preferences.layout.history}
                    onHeight={(history) =>
                      ws.updatePreferences((p) => ({ ...p, layout: { ...p.layout, history } }))
                    }
                    onError={ws.setError}
                    menuFor={commitMenu}
                  />
                ) : (
                  <Collections
                    key={view}
                    kind={view}
                    repoId={ws.repo.id}
                    refresh={ws.revision}
                    onError={ws.setError}
                    menuFor={shelfMenu}
                    onCreate={
                      view === 'stash'
                        ? stashCreate
                        : () => {
                            void worktreeCreate().catch(fail);
                          }
                    }
                    onOpen={(path) =>
                      navigate(() => {
                        void ws.openRepo(path);
                      })
                    }
                    onRemove={(path) =>
                      confirmAction(
                        '移除工作树',
                        `${path}\n仅允许移除干净的工作树，分支和提交仍保留在仓库中。`,
                        'worktreeRemove',
                        [path],
                      )
                    }
                  />
                )}
              </Suspense>
            </div>
            {logOpen && (
              <div className="operation-console">
                <div>
                  <TerminalSquare size={14} />
                  <strong>操作输出</strong>
                  <span className="flex-spacer" />
                  <IconButton title="关闭输出" onClick={() => setLogOpen(false)}>
                    <X size={14} />
                  </IconButton>
                </div>
                <div className="console-content">
                  {ws.operations.length ? (
                    ws.operations.map((operation) => (
                      <section key={operation.id}>
                        <span className={`operation-state ${operation.state}`}>
                          {operation.state === 'success'
                            ? '已完成'
                            : operation.state === 'error'
                              ? '失败'
                              : operation.state === 'cancelled'
                                ? '已取消'
                                : '执行中'}
                        </span>
                        <pre>{operation.logs}</pre>
                      </section>
                    ))
                  ) : (
                    <span className="muted">执行操作后，结果会显示在这里。</span>
                  )}
                </div>
              </div>
            )}
          </main>
        </div>
      )}

      {(ws.error || ws.notice) && (
        <div
          className={`notification ${ws.error ? 'error' : 'info'}`}
          role={ws.error ? 'alert' : 'status'}
        >
          {ws.error ? <CircleAlert size={17} /> : <Check size={17} />}
          <span>{ws.error || ws.notice}</span>
          {!ws.error && ws.createdPath && (
            <button
              className="button small-button"
              onClick={() =>
                navigate(() => {
                  void ws.openRepo(ws.createdPath!);
                  ws.setCreatedPath(null);
                })
              }
            >
              打开新仓库
            </button>
          )}
          {ws.error.includes('fast-forward') && (
            <>
              <button
                className="button small-button"
                onClick={() => {
                  ws.setError('');
                  invokeAction('pullMerge');
                }}
              >
                合并拉取
              </button>
              <button
                className="button small-button"
                onClick={() => {
                  ws.setError('');
                  rebase(false, ws.status?.upstream ?? '');
                }}
              >
                变基
              </button>
            </>
          )}
          <IconButton
            title="关闭提示"
            onClick={() => {
              ws.setError('');
              ws.setNotice('');
            }}
          >
            <X size={15} />
          </IconButton>
        </div>
      )}
      <footer className="statusbar">
        <GitBranch size={12} />
        <span>
          {ws.status?.branch === '(detached)' ? '游离 HEAD' : (ws.status?.branch ?? 'GitSprig')}
        </span>
        {ws.status?.head && <code>{shortOid(ws.status.head)}</code>}
        <span className="flex-spacer" />
        {pending ? (
          <>
            <LoaderCircle className="spin" size={12} />
            <span className="status-progress">Git 操作进行中…</span>
            <button onClick={() => void api.cancel(pending.id).catch(fail)}>取消</button>
          </>
        ) : (
          <>
            <i className="dot green" />
            <span>{ws.repo ? '已连接本地仓库' : '轻量 · 独立 · 本地'}</span>
          </>
        )}
        <span className="status-separator" />
        <span>Asia/Shanghai</span>
      </footer>
      {dialog && <Dialog key={dialog.title} spec={dialog} onClose={() => setDialog(null)} />}
      {pushOpen && ws.repo && ws.status && (
        <Suspense fallback={null}>
          <PushDialog
            key={ws.repo.id}
            repo={ws.repo}
            status={ws.status}
            references={ws.references}
            remotes={ws.remotes}
            force={pushOpen.force}
            busy={ws.busy}
            onClose={() => setPushOpen(null)}
          />
        </Suspense>
      )}
      {cloneOpen && (
        <Suspense fallback={null}>
          <CloneRepository onClose={() => setCloneOpen(false)} />
        </Suspense>
      )}
      {ai.settingsOpen && (
        <Suspense fallback={null}>
          <AiSettingsDialog onClose={() => ai.setSettingsOpen(false)} />
        </Suspense>
      )}
      {ai.suggestion && (
        <Dialog
          spec={{
            title: '生成结果 · 你的草稿已保留',
            description:
              '等待期间你修改了提交说明。确认后将用以下生成结果替换当前草稿：\n\n' +
              ai.suggestion.text,
            confirmLabel: '填入提交说明',
            submit: ai.applySuggestion,
          }}
          onClose={() => ai.setSuggestion(null)}
        />
      )}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
      {ws.prompts[0] && (
        <PromptDialog
          key={ws.prompts[0].id}
          prompt={ws.prompts[0]}
          onClose={() => ws.setPrompts((p) => p.filter((prompt) => prompt.id !== ws.prompts[0].id))}
        />
      )}
      {remoteOpen && (
        <Modal title="远程仓库" onClose={() => setRemoteOpen(false)}>
          <div className="remote-list">
            {ws.remotes.map((remote) => (
              <div key={remote.name}>
                <Globe2 size={17} />
                <span>
                  <strong>{remote.name}</strong>
                  <code>{remote.fetchUrl}</code>
                  {remote.pushUrl !== remote.fetchUrl && <small>推送：{remote.pushUrl}</small>}
                </span>
                <button
                  className="text-button"
                  onClick={() => remoteEdit(remote.name, remote.fetchUrl)}
                >
                  编辑
                </button>
                <button
                  className="text-button danger-text"
                  onClick={() => {
                    setRemoteOpen(false);
                    confirmAction(
                      '移除远程仓库',
                      `移除本地远程配置 ${remote.name}，服务器上的仓库不会被删除。`,
                      'remoteRemove',
                      [remote.name],
                    );
                  }}
                >
                  移除
                </button>
              </div>
            ))}
            {!ws.remotes.length && <Empty title="尚未配置远程仓库" />}
          </div>
          <div className="dialog-actions">
            <button className="button primary" onClick={() => remoteEdit()}>
              <Plus size={14} />
              添加远程
            </button>
          </div>
        </Modal>
      )}
      {blame && (
        <Modal title={`逐行追溯 · ${blame.path}`} wide onClose={() => setBlame(null)}>
          <Suspense fallback={<Spinner />}>
            <Blame text={blame.text} />
          </Suspense>
        </Modal>
      )}
      {palette && (
        <Modal
          title="查找操作"
          onClose={() => {
            setPalette(false);
            setPaletteSearch('');
          }}
        >
          <div className="palette-search">
            <Command size={17} />
            <input
              autoFocus
              placeholder="输入操作名称…"
              value={paletteSearch}
              onChange={(e) => setPaletteSearch(e.target.value)}
            />
          </div>
          <div className="palette-results">
            {allCommands
              .filter((command) => command.label.includes(paletteSearch))
              .map((command) => (
                <button
                  key={command.label}
                  disabled={command.disabled}
                  onClick={() => {
                    setPalette(false);
                    setPaletteSearch('');
                    command.action();
                  }}
                >
                  {command.icon ?? <span className="menu-icon-space" />}
                  <span>{command.label}</span>
                  <kbd>{command.shortcut}</kbd>
                </button>
              ))}
          </div>
        </Modal>
      )}
    </div>
  );
}
