import { useEffect, useMemo, useRef, useState } from 'react';
import {
  ChevronDown,
  ChevronRight,
  FileCode2,
  Folder,
  GitBranch,
  Globe2,
  Plus,
  Search,
  Tag,
} from 'lucide-react';
import { basename, dirname, fileStatus, fileRowRange, selectFileRows } from '../logic';
import type {
  ChangeGroup,
  FileChange,
  Reference,
  RepoGroups,
  RepositoryStatus,
  Selection,
} from '../types';
import type { MenuItem } from './UI';
import { Empty, IconButton, Menu, VirtualList } from './UI';

export interface ChangeFileRow {
  kind: 'file';
  file: FileChange;
  groupId: string;
}
const rowKey = (row: ChangeFileRow) => row.groupId + ':' + row.file.id;

export function ChangesList({
  status,
  selectedFile,
  selections,
  groups,
  onFile,
  onSetChecked,
  onNewGroup,
  fileMenu,
  groupMenu,
}: {
  status: RepositoryStatus;
  selectedFile: string | null;
  selections: Record<string, Selection>;
  groups: RepoGroups;
  onFile: (file: FileChange) => void;
  onSetChecked: (rows: ChangeFileRow[], checked?: boolean) => Promise<void>;
  onNewGroup: () => void;
  fileMenu: (file: FileChange, selected: FileChange[]) => MenuItem[];
  groupMenu: (group: ChangeGroup) => MenuItem[];
}) {
  const [search, setSearch] = useState('');
  const [collapsed, setCollapsed] = useState(new Set<string>());
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [marked, setMarked] = useState<Set<string> | null>(null);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const pendingFocus = useRef<string | null>(null);
  type Row = { kind: 'group'; group: ChangeGroup; count: number } | ChangeFileRow;
  const [scrollKey, setScrollKey] = useState<string | null>(null);
  const rows = useMemo(() => {
    const result: Row[] = [];
    const filtered = status.files.filter((file) =>
      file.path.toLowerCase().includes(search.toLowerCase()),
    );
    const allGroups = status.files.some((f) => f.conflict)
      ? [{ id: 'conflicts', name: '合并冲突' }, ...groups.groups]
      : groups.groups;
    for (const group of allGroups) {
      const files = filtered.filter((file) => {
        if (file.conflict) return group.id === 'conflicts';
        if ((groups.fileGroups[file.id] ?? 'default') === group.id) return true;
        return Object.values(groups.hunkGroups[file.id] ?? {}).some(
          (value) => value.groupId === group.id,
        );
      });
      if (group.id !== 'default' && !files.length && !groups.groups.some((g) => g.id === group.id))
        continue;
      result.push({ kind: 'group', group, count: files.length });
      if (!collapsed.has(group.id))
        files.forEach((file) => result.push({ kind: 'file', file, groupId: group.id }));
    }
    return result;
  }, [status.files, groups, search, collapsed]);
  const fileRows = useMemo(
    () => rows.filter((row): row is ChangeFileRow => row.kind === 'file'),
    [rows],
  );
  const visibleKeys = fileRows.map(rowKey);
  const initialKey = fileRows.find((row) => row.file.id === selectedFile);
  const selectedKeys = marked ?? new Set(initialKey ? [rowKey(initialKey)] : []);
  const selectedRows = fileRows.filter((row) => selectedKeys.has(rowKey(row)));
  const selectedCount = new Set(selectedRows.map((row) => row.file.id)).size;
  const scrollIndex = rows.findIndex((row) => row.kind === 'file' && rowKey(row) === scrollKey);
  useEffect(() => {
    const visible = new Set(fileRows.map(rowKey));
    setMarked((previous) => {
      if (!previous) return previous;
      const next = new Set([...previous].filter((key) => visible.has(key)));
      return next.size === previous.size ? previous : next;
    });
    setAnchor((previous) => (previous && visible.has(previous) ? previous : null));
  }, [fileRows]);
  const chooseRow = (
    row: ChangeFileRow,
    gesture: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean },
    focus = false,
  ) => {
    const key = rowKey(row);
    const next = selectFileRows(
      visibleKeys,
      selectedKeys,
      anchor ?? (initialKey ? rowKey(initialKey) : null),
      key,
      gesture,
    );
    setMarked(next.selected);
    setAnchor(next.anchor);
    onFile(row.file);
    if (focus) {
      pendingFocus.current = key;
      setScrollKey(key);
    }
  };
  const checkRows = async (targets: ChangeFileRow[], include?: boolean) => {
    if (checking) return;
    setChecking(true);
    try {
      await onSetChecked(
        targets.filter((row) => !row.file.conflict),
        include,
      );
    } finally {
      setChecking(false);
    }
  };
  const renderFile = (row: Extract<Row, { kind: 'file' }>) => {
    const selection = selections[row.file.id];
    const assignments = Object.values(groups.hunkGroups[row.file.id] ?? {});
    let checked = !!selection;
    if (selection && !selection.all && assignments.length) {
      const defaultGroup = groups.fileGroups[row.file.id] ?? 'default';
      const own = new Set(
        assignments.filter((a) => a.groupId === row.groupId).flatMap((a) => a.lineIds),
      );
      const other = new Set(
        assignments.filter((a) => a.groupId !== row.groupId).flatMap((a) => a.lineIds),
      );
      checked = selection.lineIds.some(
        (id) => own.has(id) || (row.groupId === defaultGroup && !other.has(id)),
      );
    }
    return (
      <div
        className={`file-row ${selectedKeys.has(rowKey(row)) ? 'selected' : ''}`}
        onContextMenu={(event) => {
          event.preventDefault();
          let targets = selectedRows;
          if (!selectedKeys.has(rowKey(row))) {
            setMarked(new Set([rowKey(row)]));
            setAnchor(rowKey(row));
            onFile(row.file);
            targets = [row];
          }
          const files = [...new Map(targets.map((item) => [item.file.id, item.file])).values()];
          setMenu({ x: event.clientX, y: event.clientY, items: fileMenu(row.file, files) });
        }}
        title={`${row.file.path}${row.file.staged ? '\n包含已有暂存内容' : ''}${assignments.length ? '\n已按代码块分组' : ''}`}
      >
        {!row.file.conflict ? (
          <input
            type="checkbox"
            aria-label={`选择 ${row.file.path}（${groups.groups.find((g) => g.id === row.groupId)?.name ?? row.groupId}）`}
            checked={checked}
            disabled={checking}
            onClick={(event) => event.currentTarget.focus({ preventScroll: true })}
            ref={(element) => {
              if (element) element.indeterminate = checked && !selection?.all;
            }}
            onChange={(event) => {
              const gesture = event.nativeEvent as MouseEvent;
              const range = gesture.shiftKey
                ? new Set(
                    fileRowRange(
                      visibleKeys,
                      anchor ?? (initialKey ? rowKey(initialKey) : null),
                      rowKey(row),
                    ),
                  )
                : new Set([rowKey(row)]);
              chooseRow(row, gesture);
              void checkRows(
                fileRows.filter((item) => range.has(rowKey(item))),
                gesture.shiftKey ? event.target.checked : undefined,
              );
            }}
          />
        ) : (
          <span className="conflict-indicator">!</span>
        )}
        <button
          className="file-row-open"
          aria-label={`查看差异 ${row.file.path}`}
          aria-pressed={selectedKeys.has(rowKey(row))}
          ref={(element) => {
            if (element && pendingFocus.current === rowKey(row)) {
              pendingFocus.current = null;
              element.focus({ preventScroll: true });
            }
          }}
          onClick={(event) => chooseRow(row, event, true)}
          onKeyDown={(event) => {
            if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
              event.preventDefault();
              const index = fileRows.findIndex((item) => rowKey(item) === rowKey(row));
              const nextIndex =
                event.key === 'Home'
                  ? 0
                  : event.key === 'End'
                    ? fileRows.length - 1
                    : Math.max(
                        0,
                        Math.min(fileRows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)),
                      );
              if (fileRows[nextIndex]) chooseRow(fileRows[nextIndex], event, true);
            } else if (
              (event.metaKey || event.ctrlKey) &&
              !event.shiftKey &&
              event.key.toLowerCase() === 'a'
            ) {
              event.preventDefault();
              setMarked(new Set(visibleKeys));
              setAnchor(rowKey(row));
            } else if (event.key === ' ') {
              event.preventDefault();
              void checkRows(selectedKeys.has(rowKey(row)) ? selectedRows : [row], !checked);
            } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
              event.preventDefault();
              const rect = event.currentTarget.getBoundingClientRect();
              const files = [
                ...new Map(
                  (selectedKeys.has(rowKey(row)) ? selectedRows : [row]).map((item) => [
                    item.file.id,
                    item.file,
                  ]),
                ).values(),
              ];
              setMenu({ x: rect.left + 16, y: rect.bottom, items: fileMenu(row.file, files) });
            }
          }}
        >
          <FileCode2 size={13} className={`file-${fileStatus(row.file.status)}`} />
          <span className={`file-name file-${fileStatus(row.file.status)}`}>
            {basename(row.file.path)}
          </span>
          <span className="file-dir">{dirname(row.file.path)}</span>
          {row.file.staged && <i className="staged-dot" title="包含已有暂存内容" />}
          {row.file.submodule && <span className="tiny-badge">子模块</span>}
          {assignments.some((value) => value.stale) && (
            <span className="tiny-badge danger-text">待归组</span>
          )}
        </button>
      </div>
    );
  };
  return (
    <div className="changes-list">
      <div className="panel-heading">
        <strong>本地变更</strong>
        <span className="count">{status.files.length}</span>
        {selectedCount > 1 && (
          <span className="row-selection-count" aria-live="polite">
            选中 {selectedCount} 项
          </span>
        )}
        <span className="flex-spacer" />
        <IconButton title="新建变更组" onClick={onNewGroup}>
          <Plus size={15} />
        </IconButton>
      </div>
      <div className="sidebar-search">
        <Search size={13} />
        <input
          placeholder="筛选文件…"
          aria-label="筛选改动文件"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <kbd>⌘ F</kbd>
      </div>
      {!status.files.length ? (
        <Empty icon={<CheckClean />} title="工作区干净" detail="所有改动都已妥善保存。" />
      ) : (
        <VirtualList
          items={rows}
          rowHeight={32}
          scrollToIndex={scrollIndex >= 0 ? scrollIndex : undefined}
          render={(row) =>
            row.kind === 'group' ? (
              <div
                className="change-group"
                onContextMenu={(event) => {
                  event.preventDefault();
                  if (row.group.id !== 'conflicts')
                    setMenu({ x: event.clientX, y: event.clientY, items: groupMenu(row.group) });
                }}
              >
                <button
                  onClick={() =>
                    setCollapsed((previous) => {
                      const next = new Set(previous);
                      next.has(row.group.id) ? next.delete(row.group.id) : next.add(row.group.id);
                      return next;
                    })
                  }
                >
                  {collapsed.has(row.group.id) ? (
                    <ChevronRight size={12} />
                  ) : (
                    <ChevronDown size={12} />
                  )}
                  <Folder size={13} className={row.group.id === 'conflicts' ? 'danger-text' : ''} />
                  <span>{row.group.name}</span>
                  <small>{row.count}</small>
                </button>
              </div>
            ) : (
              renderFile(row)
            )
          }
        />
      )}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

function CheckClean() {
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" fill="none">
      <circle cx="14" cy="14" r="11" stroke="currentColor" strokeWidth="1.3" />
      <path d="m8 14 4 4 8-9" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

export function BranchList({
  references,
  selected,
  onSelect,
  menuFor,
  onNew,
}: {
  references: Reference[];
  selected: string;
  onSelect: (ref: string) => void;
  menuFor: (ref: Reference) => MenuItem[];
  onNew: () => void;
}) {
  const [search, setSearch] = useState('');
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [collapsed, setCollapsed] = useState(new Set<string>());
  const filtered = references.filter((ref) =>
    ref.name.toLowerCase().includes(search.toLowerCase()),
  );
  return (
    <div className="branch-list">
      <div className="panel-heading">
        <strong>分支</strong>
        <span className="flex-spacer" />
        <IconButton title="新建分支" onClick={onNew}>
          <Plus size={15} />
        </IconButton>
      </div>
      <div className="sidebar-search">
        <Search size={13} />
        <input
          placeholder="搜索分支或标签…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
      <div className="branch-scroll">
        <button
          className={`branch-row ${selected === '' ? 'selected' : ''}`}
          onClick={() => onSelect('')}
        >
          <GitBranch size={14} />
          <span>所有分支</span>
        </button>
        {(['local', 'remote', 'tag'] as const).map((kind) => (
          <div key={kind}>
            <button
              className="branch-group-heading"
              onClick={() =>
                setCollapsed((previous) => {
                  const next = new Set(previous);
                  next.has(kind) ? next.delete(kind) : next.add(kind);
                  return next;
                })
              }
            >
              {collapsed.has(kind) ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
              {kind === 'local' ? '本地分支' : kind === 'remote' ? '远程分支' : '标签'}
              <small>{filtered.filter((ref) => ref.kind === kind).length}</small>
            </button>
            {!collapsed.has(kind) &&
              filtered
                .filter((ref) => ref.kind === kind)
                .map((ref) => (
                  <button
                    key={ref.fullName}
                    className={`branch-row ${selected === ref.fullName ? 'selected' : ''}`}
                    onClick={() => onSelect(ref.fullName)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      setMenu({ x: event.clientX, y: event.clientY, items: menuFor(ref) });
                    }}
                    title={ref.name}
                  >
                    {kind === 'remote' ? (
                      <Globe2 size={13} />
                    ) : kind === 'tag' ? (
                      <Tag size={13} />
                    ) : (
                      <GitBranch size={13} />
                    )}
                    <span className="truncate">{ref.name}</span>
                    {ref.current && <span className="current-label">当前</span>}
                  </button>
                ))}
          </div>
        ))}
      </div>
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
