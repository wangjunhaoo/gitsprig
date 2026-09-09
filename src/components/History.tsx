import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, ArrowLeftRight, FileCode2, GitCommitHorizontal, Search, X } from 'lucide-react';
import { api, errorMessage } from '../api';
import { basename, buildGraph, dirname, fileStatus, formatTime, shortOid } from '../logic';
import type { Commit, FileChange, FileDiff, Reference } from '../types';
import type { GraphRow } from '../logic';
import type { MenuItem } from './UI';
import { Empty, IconButton, Menu, ResizeHandle, Spinner, VirtualList } from './UI';

const DiffEditor = lazy(() => import('./Editor'));
const colors = ['#75a1ed', '#b990e9', '#75bc9c', '#dda967', '#dc8593', '#70b9c9', '#b4ba78'];
function Graph({ graph, width }: { graph: GraphRow; width: number }) {
  const x = (lane: number) => 13 + lane * 14;
  return (
    <svg className="commit-graph" width={width} height={32} aria-hidden="true">
      {graph.continues.map((next, index) =>
        next < 0 ? null : (
          <path
            key={`c${index}`}
            d={`M${x(index)} 0 C${x(index)} 16,${x(next)} 16,${x(next)} 32`}
            fill="none"
            stroke={colors[index % colors.length]}
            strokeWidth={1.6}
          />
        ),
      )}
      <path
        d={`M${x(graph.lane)} 0 L${x(graph.lane)} 16`}
        stroke={colors[graph.color]}
        strokeWidth={1.6}
      />
      {graph.parentLanes.map((parent, index) => (
        <path
          key={`p${index}`}
          d={`M${x(graph.lane)} 16 C${x(graph.lane)} 24,${x(parent)} 20,${x(parent)} 32`}
          fill="none"
          stroke={colors[graph.color]}
          strokeWidth={1.6}
        />
      ))}
      <circle
        cx={x(graph.lane)}
        cy={16}
        r={3.5}
        fill="var(--panel)"
        stroke={colors[graph.color]}
        strokeWidth={2}
      />
    </svg>
  );
}

export function CommitDetail({
  repoId,
  revision,
  base,
  onError,
  stash = false,
}: {
  repoId: string;
  revision: string;
  base?: string;
  onError: (error: string) => void;
  stash?: boolean;
}) {
  const [files, setFiles] = useState<FileChange[]>([]);
  const [file, setFile] = useState<FileChange | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [loading, setLoading] = useState(false);
  const [diffLoading, setDiffLoading] = useState(false);
  const [parents, setParents] = useState<string[]>([]);
  const [variant, setVariant] = useState('worktree');
  const [message, setMessage] = useState('');
  const fullGeneration = useRef(0);
  const actualRevision =
    stash && variant === 'index'
      ? parents[1]
      : stash && variant === 'untracked'
        ? parents[2]
        : revision;
  useEffect(() => {
    let alive = true;
    setVariant('worktree');
    void Promise.all([
      api.history(repoId, { revision, skip: 0, limit: 1 }),
      api.message(repoId, revision),
    ])
      .then(([commits, message]) => {
        if (alive) {
          setParents(commits[0]?.parents ?? []);
          setMessage(message);
        }
      })
      .catch((e) => onError(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, [repoId, revision]);
  useEffect(() => {
    if (!actualRevision) return;
    let alive = true;
    setLoading(true);
    setFiles([]);
    setFile(null);
    setDiff(null);
    void api
      .commitFiles(repoId, actualRevision, base)
      .then((files) => {
        if (alive) {
          setFiles(files);
          setFile(files[0] ?? null);
        }
      })
      .catch((e) => {
        if (alive) onError(errorMessage(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [repoId, actualRevision, base]);
  useEffect(() => {
    fullGeneration.current++;
    if (!file || !actualRevision) return;
    let alive = true;
    setDiffLoading(true);
    setDiff(null);
    void api
      .diff(repoId, file.id, base, actualRevision)
      .then((diff) => {
        if (alive) setDiff(diff);
      })
      .catch((e) => {
        if (alive) onError(errorMessage(e));
      })
      .finally(() => {
        if (alive) setDiffLoading(false);
      });
    return () => {
      alive = false;
      fullGeneration.current++;
    };
  }, [repoId, file?.id, actualRevision, base]);
  return (
    <div className="commit-detail">
      <div className="detail-file-list">
        <div className="panel-heading">
          <span>已更改文件</span>
          <span className="count">{files.length}</span>
        </div>
        {stash && (
          <select
            className="stash-variant"
            aria-label="Stash 内容"
            value={variant}
            onChange={(e) => setVariant(e.target.value)}
          >
            <option value="worktree">工作区改动</option>
            {parents[1] && <option value="index">原暂存区改动</option>}
            {parents[2] && <option value="untracked">未跟踪文件</option>}
          </select>
        )}
        {loading ? (
          <Spinner />
        ) : (
          <VirtualList
            items={files}
            rowHeight={45}
            render={(item) => (
              <button
                className={`detail-file-row ${file?.id === item.id ? 'selected' : ''}`}
                title={item.path}
                onClick={() => setFile(item)}
              >
                <FileCode2 size={14} className={`file-${fileStatus(item.status)}`} />
                <span>
                  <span>{basename(item.path)}</span>
                  <small>{dirname(item.path) || '/'}</small>
                </span>
                <i className={`status-letter file-${fileStatus(item.status)}`}>{item.status[0]}</i>
              </button>
            )}
          />
        )}
        <div className="commit-message-preview">
          <code>{shortOid(revision)}</code>
          <p>{message}</p>
        </div>
      </div>
      <div className="detail-diff">
        {file && (
          <div className="file-breadcrumb">
            <FileCode2 size={14} />
            <span>{file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}</span>
            {base && (
              <code>
                {shortOid(base)} → {shortOid(revision)}
              </code>
            )}
          </div>
        )}
        {diffLoading ? (
          <Spinner />
        ) : diff ? (
          <Suspense fallback={<Spinner />}>
            <DiffEditor
              diff={diff}
              readonly
              onFull={() => {
                if (file && actualRevision) {
                  const generation = ++fullGeneration.current;
                  setDiffLoading(true);
                  void api
                    .diff(repoId, file.id, base, actualRevision, true)
                    .then((value) => {
                      if (generation === fullGeneration.current) setDiff(value);
                    })
                    .catch((e) => {
                      if (generation === fullGeneration.current) onError(errorMessage(e));
                    })
                    .finally(() => {
                      if (generation === fullGeneration.current) setDiffLoading(false);
                    });
                }
              }}
            />
          </Suspense>
        ) : (
          <Empty
            icon={<GitCommitHorizontal size={30} />}
            title={loading ? '读取提交内容…' : '没有文件差异'}
            detail="选择一个文件查看具体改动。"
          />
        )}
      </div>
    </div>
  );
}

export default function History({
  repoId,
  references,
  revisionFilter,
  onRevisionFilter,
  pathFilter,
  refresh,
  height,
  onHeight,
  onError,
  menuFor,
}: {
  repoId: string;
  references: Reference[];
  revisionFilter: string;
  onRevisionFilter: (revision: string) => void;
  refresh: number;
  height: number;
  pathFilter: string;
  onHeight: (height: number) => void;
  onError: (error: string) => void;
  menuFor: (commit: Commit) => MenuItem[];
}) {
  const [commits, setCommits] = useState<Commit[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [search, setSearch] = useState('');
  const [author, setAuthor] = useState('');
  const [path, setPath] = useState('');
  useEffect(() => {
    setPath(pathFilter);
  }, [pathFilter]);
  const [loading, setLoading] = useState(false);
  const [more, setMore] = useState(true);
  const [menu, setMenu] = useState<{ x: number; y: number; commit: Commit } | null>(null);
  const generation = useRef(0);
  const query = {
    revision: revisionFilter || undefined,
    author: author || undefined,
    search: search || undefined,
    path: path || undefined,
    skip: 0,
    limit: 200,
  };
  useEffect(() => {
    const current = ++generation.current;
    const timer = setTimeout(
      () => {
        setLoading(true);
        void api
          .history(repoId, query)
          .then((rows) => {
            if (generation.current === current) {
              setCommits(rows);
              setMore(rows.length === 200);
              setSelected((previous) =>
                previous.filter((oid) => rows.some((row) => row.oid === oid)),
              );
            }
          })
          .catch((e) => onError(errorMessage(e)))
          .finally(() => {
            if (generation.current === current) setLoading(false);
          });
      },
      search || author || path ? 250 : 0,
    );
    return () => {
      clearTimeout(timer);
    };
  }, [repoId, revisionFilter, search, author, path, refresh]);
  const graphs = useMemo(() => buildGraph(commits), [commits]);
  const graphWidth = Math.max(
    70,
    Math.min(300, 20 + Math.max(0, ...graphs.map((g) => g.before.length)) * 14),
  );
  const style = { gridTemplateColumns: `${graphWidth}px minmax(180px,1fr) 125px 92px 112px` };
  const loadMore = async () => {
    const current = generation.current;
    setLoading(true);
    try {
      const rows = await api.history(repoId, { ...query, skip: commits.length });
      if (current === generation.current) {
        setCommits((previous) => [...previous, ...rows]);
        setMore(rows.length === 200);
      }
    } catch (e) {
      if (current === generation.current) onError(errorMessage(e));
    } finally {
      if (current === generation.current) setLoading(false);
    }
  };
  const selectedRevision = selected.at(-1);
  const selectedBase = selected.length === 2 ? selected[0] : undefined;
  return (
    <div className="history-view">
      <div className="history-top" style={{ height }}>
        <div className="history-filters">
          <div className="search-input">
            <Search size={14} />
            <input
              placeholder="搜索提交说明…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <select
            aria-label="筛选分支"
            value={revisionFilter}
            onChange={(e) => onRevisionFilter(e.target.value)}
          >
            <option value="">所有分支</option>
            <option value="HEAD">当前分支</option>
            {references.map((ref) => (
              <option key={ref.fullName} value={ref.fullName}>
                {ref.name}
              </option>
            ))}
          </select>
          <input
            className="filter-field"
            placeholder="作者"
            aria-label="筛选作者"
            value={author}
            onChange={(e) => setAuthor(e.target.value)}
          />
          <input
            className="filter-field path-filter"
            placeholder="文件路径"
            aria-label="筛选文件路径"
            value={path}
            onChange={(e) => setPath(e.target.value)}
          />
          {(search || author || path) && (
            <IconButton
              title="清除筛选"
              onClick={() => {
                setSearch('');
                setAuthor('');
                setPath('');
              }}
            >
              <X size={14} />
            </IconButton>
          )}
        </div>
        <div className="history-columns" style={style}>
          <span>分支图</span>
          <span>提交说明</span>
          <span>作者</span>
          <span>提交</span>
          <span>时间 · 上海</span>
        </div>
        {!commits.length && loading ? (
          <Spinner />
        ) : !commits.length ? (
          <Empty
            icon={<GitCommitHorizontal size={30} />}
            title="没有匹配的提交"
            detail="可以调整筛选，或创建你的第一次提交。"
          />
        ) : (
          <VirtualList
            items={commits}
            rowHeight={32}
            className="history-list"
            render={(commit, index) => (
              <button
                className={`history-row ${selected.includes(commit.oid) ? 'selected' : ''}`}
                style={style}
                onClick={(event) =>
                  setSelected((previous) =>
                    event.metaKey || event.ctrlKey
                      ? previous.includes(commit.oid)
                        ? previous.filter((v) => v !== commit.oid)
                        : [...previous, commit.oid].slice(-2)
                      : [commit.oid],
                  )
                }
                onContextMenu={(event) => {
                  event.preventDefault();
                  setMenu({ x: event.clientX, y: event.clientY, commit });
                }}
                title={commit.subject}
              >
                <Graph graph={graphs[index]} width={graphWidth} />
                <span className="commit-subject">
                  {commit.decorations && (
                    <span className="ref-badge">
                      {commit.decorations.replace('HEAD -> ', '').split(', ')[0]}
                    </span>
                  )}
                  {commit.subject}
                </span>
                <span className="truncate muted">{commit.author}</span>
                <code>{shortOid(commit.oid)}</code>
                <time>{formatTime(commit.timestamp)}</time>
              </button>
            )}
          />
        )}
        <div className="history-footer">
          <span>
            {commits.length} 条提交{loading && ' · 正在读取…'}
          </span>
          <span className="flex-spacer" />
          <span>⌘ 单击两条提交进行对比</span>
          {more && (
            <button className="text-button" disabled={loading} onClick={() => void loadMore()}>
              <ArrowDown size={12} />
              加载更多
            </button>
          )}
        </div>
      </div>
      <ResizeHandle value={height} onChange={onHeight} direction="vertical" min={180} max={600} />
      {selectedRevision ? (
        <div className="history-bottom">
          {selectedBase && (
            <div className="compare-heading">
              <ArrowLeftRight size={14} />
              <span>
                对比 {shortOid(selectedBase)} → {shortOid(selectedRevision)}
              </span>
              <button
                className="text-button"
                onClick={() => setSelected([selectedRevision, selectedBase])}
              >
                交换版本
              </button>
            </div>
          )}
          <CommitDetail
            repoId={repoId}
            revision={selectedRevision}
            base={selectedBase}
            onError={onError}
          />
        </div>
      ) : (
        <Empty
          icon={<GitCommitHorizontal size={32} />}
          title="每次提交的细节，都在这里"
          detail="选择上方提交查看文件改动，按住 ⌘ 选择两条提交进行比较。"
        />
      )}
      {menu && (
        <Menu x={menu.x} y={menu.y} items={menuFor(menu.commit)} onClose={() => setMenu(null)} />
      )}
    </div>
  );
}
