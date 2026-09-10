import { useEffect, useRef, useState } from 'react';
import { EditorView } from '@codemirror/view';
import { presentableDiff } from '@codemirror/merge';
import {
  ArrowLeft,
  ArrowRight,
  CheckCheck,
  ChevronDown,
  ChevronUp,
  GitMerge,
  RotateCcw,
  Save,
} from 'lucide-react';
import { api, errorMessage } from '../api';
import { acceptConflict, parseConflicts } from '../logic';
import type { ConflictFile } from '../types';
import { CodeText } from './Editor';
import { Empty, IconButton, Modal, Spinner } from './UI';

export default function MergeEditor({
  repoId,
  fileId,
  onResolved,
  onError,
  onDirtyChange,
}: {
  repoId: string;
  fileId: string;
  onResolved: () => void;
  onError: (message: string) => void;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const [conflict, setConflict] = useState<ConflictFile | null>(null);
  const [content, setContent] = useState('');
  const [saved, setSaved] = useState('');
  const [busy, setBusy] = useState(false);
  const [baseOpen, setBaseOpen] = useState(false);
  const [current, setCurrent] = useState(0);
  const epoch = useRef(0);
  const views = useRef<(EditorView | null)[]>([null, null, null]);
  const contentRef = useRef(content);
  contentRef.current = content;
  const dirty = content !== saved;
  useEffect(() => {
    onDirtyChange(dirty);
  }, [dirty, onDirtyChange]);
  const blocks = parseConflicts(content);
  const index = Math.min(current, Math.max(0, blocks.length - 1));
  const block = blocks[index];

  const load = async () => {
    const generation = epoch.current;
    const data = await api.conflict(repoId, fileId);
    if (generation !== epoch.current) return;
    setConflict(data);
    setContent(data.result ?? '');
    setSaved(data.result ?? '');
    setCurrent(0);
  };
  useEffect(() => {
    const generation = ++epoch.current;
    setConflict(null);
    setBusy(false);
    void api
      .conflict(repoId, fileId)
      .then((data) => {
        if (generation === epoch.current) {
          setConflict(data);
          setContent(data.result ?? '');
          setSaved(data.result ?? '');
          setCurrent(0);
        }
      })
      .catch((e) => {
        if (generation === epoch.current) onError(errorMessage(e));
      });
    return () => {
      epoch.current++;
    };
  }, [repoId, fileId]);
  useEffect(() => {
    if (!dirty) return;
    const prevent = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', prevent);
    return () => window.removeEventListener('beforeunload', prevent);
  }, [dirty]);

  useEffect(() => {
    if (!conflict || conflict.binary) return;
    const editors = views.current;
    if (editors.some((view) => !view)) return;
    let sync = false;
    const handlers = editors.map((source, sourceIndex) => {
      const handler = () => {
        if (sync || !source) return;
        sync = true;
        const sourcePos = source.lineBlockAtHeight(source.scrollDOM.scrollTop).from;
        editors.forEach((target, targetIndex) => {
          if (!target || sourceIndex === targetIndex) return;
          const from = source.state.doc.toString(),
            to = target.state.doc.toString();
          const changes = presentableDiff(from, to, { timeout: 40, scanLimit: 500 });
          let mapped = sourcePos;
          let delta = 0;
          for (const change of changes) {
            if (sourcePos < change.fromA) break;
            if (sourcePos <= change.toA) {
              mapped = change.fromB;
              delta = 0;
              break;
            }
            delta = change.toB - change.toA;
            mapped = sourcePos + delta;
          }
          const position = Math.max(0, Math.min(target.state.doc.length, mapped));
          target.scrollDOM.scrollTop = target.lineBlockAt(position).top;
        });
        requestAnimationFrame(() => {
          sync = false;
        });
      };
      source?.scrollDOM.addEventListener('scroll', handler);
      return handler;
    });
    return () =>
      editors.forEach((view, i) => view?.scrollDOM.removeEventListener('scroll', handlers[i]));
  }, [conflict?.fileId, conflict?.binary]);

  const save = async () => {
    if (!conflict) return;
    const generation = epoch.current;
    setBusy(true);
    try {
      const resultHash = await api.saveConflict(repoId, fileId, conflict.resultHash, content);
      if (generation !== epoch.current) return;
      setConflict((c) => (c ? { ...c, resultHash } : c));
      setSaved(content);
    } catch (e) {
      if (generation === epoch.current) onError(errorMessage(e));
    } finally {
      if (generation === epoch.current) setBusy(false);
    }
  };
  const choose = async (side: string) => {
    if (!conflict) return;
    const generation = epoch.current;
    setBusy(true);
    try {
      await api.chooseConflict(repoId, fileId, conflict.resultHash, side);
      if (generation === epoch.current) await load();
    } catch (e) {
      if (generation === epoch.current) onError(errorMessage(e));
    } finally {
      if (generation === epoch.current) setBusy(false);
    }
  };
  const resolve = async () => {
    if (!conflict || dirty) return;
    const generation = epoch.current;
    setBusy(true);
    try {
      await api.markConflict(repoId, fileId, conflict.resultHash);
      if (generation === epoch.current) onResolved();
    } catch (e) {
      if (generation === epoch.current) onError(errorMessage(e));
    } finally {
      if (generation === epoch.current) setBusy(false);
    }
  };
  const accept = (side: 'ours' | 'theirs' | 'both') => {
    if (!block) return;
    setContent((text) => acceptConflict(text, block, side));
    const editor = views.current[1];
    if (editor)
      editor.dispatch({
        effects: EditorView.scrollIntoView(Math.min(block.start, editor.state.doc.length), {
          y: 'center',
        }),
      });
  };
  const navigate = (delta: number) => {
    const target = (index + delta + blocks.length) % blocks.length;
    setCurrent(target);
    const editor = views.current[1];
    if (editor && blocks[target])
      editor.dispatch({
        effects: EditorView.scrollIntoView(
          Math.min(blocks[target].start, editor.state.doc.length),
          { y: 'center' },
        ),
      });
  };

  if (!conflict) return <Spinner text="正在读取冲突的三个版本…" />;
  return (
    <div className="merge-editor">
      <div className="merge-toolbar">
        <GitMerge size={16} />
        <strong>解决冲突</strong>
        <span className="conflict-count">
          {blocks.length ? `剩余 ${blocks.length} 处` : '请检查合并结果'}
        </span>
        <span className="flex-spacer" />
        <button className="text-button" onClick={() => setBaseOpen(true)}>
          共同祖先
        </button>
        <IconButton
          title="重新加载"
          disabled={dirty || busy}
          onClick={() => void load().catch((e) => onError(errorMessage(e)))}
        >
          <RotateCcw size={14} />
        </IconButton>
        <button
          className="button"
          disabled={!dirty || busy || conflict.binary}
          onClick={() => void save()}
        >
          <Save size={14} />
          保存结果
        </button>
        <button
          className="button primary"
          disabled={dirty || busy || blocks.length > 0}
          onClick={() => void resolve()}
        >
          <CheckCheck size={14} />
          标记解决
        </button>
      </div>
      {conflict.binary || !conflict.oursExists || !conflict.theirsExists ? (
        <div className="file-conflict">
          <Empty
            icon={<GitMerge size={28} />}
            title={conflict.binary ? '按整个文件解决冲突' : '一侧文件已被删除'}
            detail="先保存要保留的版本，检查后再标记解决。"
          />
          <div className="file-conflict-actions">
            <button className="button" disabled={busy} onClick={() => void choose('ours')}>
              {conflict.oursExists ? '保留左侧版本' : '采用左侧删除'}
            </button>
            <button className="button" disabled={busy} onClick={() => void choose('theirs')}>
              {conflict.theirsExists ? '保留右侧版本' : '采用右侧删除'}
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="merge-pane-labels">
            <span>
              <i className="dot blue" />
              {conflict.oursLabel}
            </span>
            <span>
              <i className="dot green" />
              合并结果{dirty && <small>未保存</small>}
            </span>
            <span>
              <i className="dot orange" />
              {conflict.theirsLabel}
            </span>
          </div>
          <div className="merge-panes">
            <CodeText
              path={conflict.path}
              content={conflict.ours ?? ''}
              onView={(view) => {
                views.current[0] = view;
              }}
            />
            <CodeText
              path={conflict.path}
              content={content}
              editable
              onChange={setContent}
              onView={(view) => {
                views.current[1] = view;
              }}
            />
            <CodeText
              path={conflict.path}
              content={conflict.theirs ?? ''}
              onView={(view) => {
                views.current[2] = view;
              }}
            />
          </div>
          <div className="merge-bottom">
            <IconButton title="上一个冲突" disabled={!blocks.length} onClick={() => navigate(-1)}>
              <ChevronUp size={15} />
            </IconButton>
            <span>{blocks.length ? `${index + 1} / ${blocks.length}` : '所有区块已处理'}</span>
            <IconButton title="下一个冲突" disabled={!blocks.length} onClick={() => navigate(1)}>
              <ChevronDown size={15} />
            </IconButton>
            <span className="toolbar-divider" />
            <button className="button" disabled={!block} onClick={() => accept('ours')}>
              <ArrowRight size={14} />
              接受左侧
            </button>
            <button className="button" disabled={!block} onClick={() => accept('theirs')}>
              <ArrowLeft size={14} />
              接受右侧
            </button>
            <button className="button" disabled={!block} onClick={() => accept('both')}>
              保留双方（左后接右）
            </button>
            <span className="flex-spacer" />
            <span className="muted small">{dirty ? '保存后可标记解决' : '中间区域可直接编辑'}</span>
          </div>
        </>
      )}
      {baseOpen && (
        <Modal title="共同祖先" wide onClose={() => setBaseOpen(false)}>
          <div className="base-preview">
            {conflict.base === null ? (
              <Empty title="没有可显示的共同祖先文本" />
            ) : (
              <CodeText content={conflict.base} path={conflict.path} />
            )}
          </div>
        </Modal>
      )}
    </div>
  );
}
