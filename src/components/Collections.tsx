import { useEffect, useState } from 'react';
import { Archive, Clock3, ExternalLink, FolderGit2, Plus, Trash2 } from 'lucide-react';
import { api, errorMessage } from '../api';
import { formatTime, shortOid } from '../logic';
import type { ShelfEntry, Worktree } from '../types';
import type { MenuItem } from './UI';
import { CommitDetail } from './History';
import { Empty, Menu, Spinner, VirtualList } from './UI';

export default function Collections({
  kind,
  repoId,
  refresh,
  onError,
  menuFor,
  onCreate,
  onOpen,
  onRemove,
}: {
  kind: 'stash' | 'worktree' | 'reflog';
  repoId: string;
  refresh: number;
  onError: (error: string) => void;
  menuFor: (entry: ShelfEntry) => MenuItem[];
  onCreate: () => void;
  onOpen: (path: string) => void;
  onRemove: (path: string) => void;
}) {
  const [entries, setEntries] = useState<ShelfEntry[]>([]);
  const [trees, setTrees] = useState<Worktree[]>([]);
  const [selected, setSelected] = useState<ShelfEntry | null>(null);
  const [loading, setLoading] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    const load =
      kind === 'worktree'
        ? api.worktrees(repoId).then((rows) => {
            if (alive) setTrees(rows);
          })
        : (kind === 'stash' ? api.stash(repoId) : api.reflog(repoId)).then((rows) => {
            if (alive) {
              setEntries(rows);
              setSelected((current) => rows.find((row) => row.oid === current?.oid) ?? null);
            }
          });
    void load
      .catch((e) => onError(errorMessage(e)))
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [kind, repoId, refresh]);
  const title = kind === 'stash' ? '暂存工作' : kind === 'worktree' ? '工作树' : '操作历史';
  const icon =
    kind === 'stash' ? (
      <Archive size={17} />
    ) : kind === 'worktree' ? (
      <FolderGit2 size={17} />
    ) : (
      <Clock3 size={17} />
    );
  return (
    <div className="collections-view">
      <div className="collection-toolbar">
        {icon}
        <strong>{title}</strong>
        <span className="muted small">
          {kind === 'stash'
            ? '随时放下，也能随时继续'
            : kind === 'worktree'
              ? '在独立目录中同时处理不同分支'
              : '通过 Reflog 找回曾经的提交'}
        </span>
        <span className="flex-spacer" />
        {kind !== 'reflog' && (
          <button className="button" onClick={onCreate}>
            <Plus size={14} />
            {kind === 'stash' ? '保存工作' : '添加工作树'}
          </button>
        )}
      </div>
      {kind === 'worktree' ? (
        <div className="worktree-page">
          {loading ? (
            <Spinner />
          ) : (
            trees.map((tree) => (
              <div className="worktree-row" key={tree.worktree}>
                <div className="worktree-icon">
                  <FolderGit2 size={22} />
                </div>
                <div>
                  <strong>{tree.branch?.replace('refs/heads/', '') ?? '游离 HEAD'}</strong>
                  <p>{tree.worktree}</p>
                  <small>
                    <code>{shortOid(tree.HEAD)}</code>
                    {tree.locked && ' · 已锁定'}
                    {tree.prunable && ' · 可清理'}
                  </small>
                </div>
                <span className="flex-spacer" />
                <button className="button" onClick={() => onOpen(tree.worktree)}>
                  <ExternalLink size={14} />
                  打开
                </button>
                <button
                  className="button quiet danger-text"
                  title="移除工作树"
                  onClick={() => onRemove(tree.worktree)}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))
          )}
        </div>
      ) : (
        <>
          <div className="collection-list">
            {loading && !entries.length ? (
              <Spinner />
            ) : !entries.length ? (
              <Empty
                icon={icon}
                title={kind === 'stash' ? '还没有暂存的工作' : '还没有操作记录'}
                detail={
                  kind === 'stash'
                    ? '切换任务前，将未完成的改动保存到 Stash。'
                    : '仓库发生引用变化后，记录会出现在这里。'
                }
              />
            ) : (
              <VirtualList
                items={entries}
                rowHeight={48}
                render={(entry) => (
                  <button
                    className={`collection-row ${selected?.oid === entry.oid ? 'selected' : ''}`}
                    onClick={() => setSelected(entry)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      setMenu({ x: event.clientX, y: event.clientY, items: menuFor(entry) });
                    }}
                  >
                    <code>{entry.name}</code>
                    <span className="truncate">{entry.subject}</span>
                    <code className="muted">{shortOid(entry.oid)}</code>
                    <time>{formatTime(Number(entry.timestamp))}</time>
                  </button>
                )}
              />
            )}
          </div>
          <div className="collection-detail">
            {selected ? (
              <>
                <div className="collection-selection">
                  <span>{selected.subject}</span>
                  <button
                    className="text-button"
                    onClick={(event) => {
                      const bounds = event.currentTarget.getBoundingClientRect();
                      setMenu({ x: bounds.left, y: bounds.bottom + 4, items: menuFor(selected) });
                    }}
                  >
                    操作…
                  </button>
                </div>
                <CommitDetail
                  repoId={repoId}
                  revision={selected.oid}
                  stash={kind === 'stash'}
                  onError={onError}
                />
              </>
            ) : (
              <Empty
                icon={icon}
                title="选择一条记录查看改动"
                detail="右键菜单提供恢复与管理操作。"
              />
            )}
          </div>
        </>
      )}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </div>
  );
}
