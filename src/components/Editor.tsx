import { useEffect, useRef, useState } from 'react';
import { Compartment, EditorState } from '@codemirror/state';
import type { Extension } from '@codemirror/state';
import {
  EditorView,
  GutterMarker,
  gutter,
  highlightActiveLine,
  keymap,
  lineNumbers,
} from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { searchKeymap } from '@codemirror/search';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { goToNextChunk, goToPreviousChunk, MergeView, unifiedMergeView } from '@codemirror/merge';
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronsUpDown,
  Columns2,
  FileWarning,
  ListChecks,
  Rows3,
  WrapText,
} from 'lucide-react';
import { changedIds } from '../logic';
import type { FileDiff, Selection } from '../types';
import { Empty, IconButton } from './UI';

const editorTheme = EditorView.theme({
  '&': { height: '100%', fontSize: '12px', backgroundColor: 'var(--editor)', color: 'var(--text)' },
  '.cm-scroller': { overflow: 'auto', fontFamily: 'var(--mono)', lineHeight: '1.8' },
  '.cm-content': { padding: '12px 0' },
  '.cm-line': { padding: '0 14px 0 8px' },
  '.cm-gutters': {
    backgroundColor: 'var(--editor)',
    color: 'var(--muted)',
    border: 'none',
    fontSize: '11px',
  },
  '.cm-lineNumbers .cm-gutterElement': { minWidth: '36px', padding: '0 8px 0 6px' },
  '&.cm-focused': { outline: 'none' },
  '.cm-cursor': { borderLeftColor: 'var(--text)' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': {
    backgroundColor: 'var(--selection)',
  },
  '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--hover)' },
  '.cm-searchMatch': { backgroundColor: '#e2ae5540' },
  '.cm-panels': { backgroundColor: 'var(--panel)', color: 'var(--text)' },
  '.cm-textfield': {
    backgroundColor: 'var(--input)',
    color: 'var(--text)',
    border: '1px solid var(--border)',
  },
  '.cm-button': {
    background: 'var(--button)',
    color: 'var(--text)',
    border: '1px solid var(--border)',
  },
});

const syntaxTheme = HighlightStyle.define([
  { tag: tags.keyword, color: 'var(--syntax-keyword)' },
  { tag: [tags.string, tags.special(tags.string)], color: 'var(--syntax-string)' },
  { tag: [tags.number, tags.bool, tags.null], color: 'var(--syntax-number)' },
  {
    tag: [tags.function(tags.variableName), tags.function(tags.propertyName)],
    color: 'var(--syntax-function)',
  },
  { tag: [tags.typeName, tags.className], color: 'var(--syntax-type)' },
  { tag: [tags.variableName, tags.propertyName], color: 'var(--text)' },
  { tag: [tags.comment, tags.meta], color: 'var(--syntax-comment)' },
  { tag: tags.heading, color: 'var(--accent-soft)', fontWeight: '600' },
  { tag: tags.link, color: 'var(--accent-soft)', textDecoration: 'underline' },
  { tag: tags.strong, fontWeight: '600' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.invalid, color: 'var(--red)' },
]);

async function language(path: string): Promise<Extension> {
  const extension = path.split('.').at(-1)?.toLowerCase();
  switch (extension) {
    case 'ts':
    case 'tsx':
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs':
      return (await import('@codemirror/lang-javascript')).javascript({
        typescript: extension.startsWith('t'),
        jsx: extension.endsWith('x'),
      });
    case 'json':
      return (await import('@codemirror/lang-json')).json();
    case 'rs':
      return (await import('@codemirror/lang-rust')).rust();
    case 'py':
      return (await import('@codemirror/lang-python')).python();
    case 'md':
      return (await import('@codemirror/lang-markdown')).markdown();
    case 'css':
      return (await import('@codemirror/lang-css')).css();
    case 'html':
    case 'vue':
    case 'svelte':
      return (await import('@codemirror/lang-html')).html();
    default:
      return [];
  }
}

function extensions(content: string, editable: boolean): Extension[] {
  return [
    editorTheme,
    lineNumbers(),
    syntaxHighlighting(syntaxTheme),
    EditorState.readOnly.of(!editable),
    EditorView.editable.of(editable),
    EditorState.lineSeparator.of(content.includes('\r\n') ? '\r\n' : '\n'),
    keymap.of([...defaultKeymap, ...searchKeymap, ...historyKeymap]),
    ...(editable ? [history(), highlightActiveLine()] : []),
  ];
}

export function CodeText({
  content,
  path,
  editable = false,
  onChange,
  onView,
}: {
  content: string;
  path: string;
  editable?: boolean;
  onChange?: (value: string) => void;
  onView?: (view: EditorView | null) => void;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const changeRef = useRef(onChange);
  changeRef.current = onChange;
  const callbackRef = useRef(onView);
  callbackRef.current = onView;
  useEffect(() => {
    if (!parent.current) return;
    const syntax = new Compartment();
    const editor = new EditorView({
      parent: parent.current,
      doc: content,
      extensions: [
        ...extensions(content, editable),
        syntax.of([]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) changeRef.current?.(update.state.sliceDoc());
        }),
      ],
    });
    view.current = editor;
    callbackRef.current?.(editor);
    let alive = true;
    void language(path).then((extension) => {
      if (alive) editor.dispatch({ effects: syntax.reconfigure(extension) });
    });
    return () => {
      alive = false;
      callbackRef.current?.(null);
      view.current = null;
      editor.destroy();
    };
  }, [path, editable]);
  useEffect(() => {
    const editor = view.current;
    if (editor && editor.state.sliceDoc() !== content)
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: content } });
  }, [content]);
  return <div className="code-text" ref={parent} />;
}

class CheckMarker extends GutterMarker {
  constructor(
    readonly id: string,
    readonly checked: boolean,
    readonly toggle: (ids: string[]) => void,
  ) {
    super();
  }
  eq(other: CheckMarker) {
    return this.id === other.id && this.checked === other.checked;
  }
  toDOM() {
    const input = document.createElement('input');
    input.type = 'checkbox';
    input.checked = this.checked;
    input.className = 'line-checkbox';
    input.setAttribute('aria-label', '将此行加入提交');
    input.addEventListener('change', () => this.toggle([this.id]));
    return input;
  }
}

export default function DiffEditor({
  diff,
  selection,
  onToggle,
  onFull,
  readonly = false,
  onGroupHunk,
  groups,
  hunkGroups,
}: {
  diff: FileDiff;
  selection?: Selection;
  onToggle?: (ids: string[]) => void;
  onFull: () => void;
  readonly?: boolean;
  onGroupHunk?: (hunkId: string, groupId: string) => void;
  groups?: { id: string; name: string }[];
  hunkGroups?: Record<string, { groupId: string }>;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const views = useRef<EditorView[]>([]);
  const gutterCompartments = useRef<Compartment[]>([]);
  const toggleRef = useRef(onToggle);
  toggleRef.current = onToggle;
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const [mode, setMode] = useState<'split' | 'unified'>('split');
  const [wrap, setWrap] = useState(false);
  const [collapse, setCollapse] = useState(true);
  const [selectPanel, setSelectPanel] = useState(false);
  const [hunkIndex, setHunkIndex] = useState(0);
  const allIds = changedIds(diff.hunks);
  const selected = new Set(selection?.all ? allIds : (selection?.lineIds ?? []));
  const hunk = diff.hunks[Math.min(hunkIndex, diff.hunks.length - 1)];
  const makeGutter = (side: 'old' | 'new') => {
    if (readonly) return [];
    const mapping = new Map<number, string>();
    diff.hunks.forEach((h) =>
      h.lines.forEach((line) => {
        if (side === 'old' && line.kind === 'delete' && line.oldLine)
          mapping.set(line.oldLine, line.id);
        if (side === 'new' && line.kind === 'insert' && line.newLine)
          mapping.set(line.newLine, line.id);
      }),
    );
    return gutter({
      class: 'commit-gutter',
      lineMarker: (view, line) => {
        const id = mapping.get(view.state.doc.lineAt(line.from).number);
        if (!id) return null;
        const current = selectionRef.current;
        return new CheckMarker(id, !!current?.all || !!current?.lineIds.includes(id), (ids) =>
          toggleRef.current?.(ids),
        );
      },
      lineMarkerChange: () => true,
    });
  };

  useEffect(() => {
    if (!parent.current || diff.oldText === null || diff.newText === null) return;
    parent.current.replaceChildren();
    const old = diff.oldText,
      current = diff.newText;
    const syntaxA = new Compartment(),
      syntaxB = new Compartment();
    const gutterA = new Compartment(),
      gutterB = new Compartment();
    gutterCompartments.current = mode === 'split' ? [gutterA, gutterB] : [gutterB];
    let merge: MergeView | undefined;
    let editor: EditorView | undefined;
    const collapseUnchanged = collapse ? { margin: 4, minSize: 16 } : undefined;
    if (mode === 'split') {
      merge = new MergeView({
        parent: parent.current,
        a: {
          doc: old,
          extensions: [
            ...extensions(old, false),
            syntaxA.of([]),
            gutterA.of(makeGutter('old')),
            ...(wrap ? [EditorView.lineWrapping] : []),
          ],
        },
        b: {
          doc: current,
          extensions: [
            ...extensions(current, false),
            syntaxB.of([]),
            gutterB.of(makeGutter('new')),
            ...(wrap ? [EditorView.lineWrapping] : []),
          ],
        },
        highlightChanges: true,
        gutter: true,
        collapseUnchanged,
        diffConfig: { timeout: 200, scanLimit: 1500 },
      });
      views.current = [merge.a, merge.b];
      let syncing = false;
      const sync = (source: EditorView, target: EditorView) => {
        if (syncing) return;
        syncing = true;
        target.scrollDOM.scrollTop = source.scrollDOM.scrollTop;
        target.scrollDOM.scrollLeft = source.scrollDOM.scrollLeft;
        requestAnimationFrame(() => {
          syncing = false;
        });
      };
      merge.a.scrollDOM.addEventListener('scroll', () => sync(merge!.a, merge!.b));
      merge.b.scrollDOM.addEventListener('scroll', () => sync(merge!.b, merge!.a));
    } else {
      editor = new EditorView({
        parent: parent.current,
        doc: current,
        extensions: [
          ...extensions(current, false),
          syntaxB.of([]),
          gutterB.of(makeGutter('new')),
          unifiedMergeView({
            original: old,
            highlightChanges: true,
            mergeControls: false,
            collapseUnchanged,
            diffConfig: { timeout: 200, scanLimit: 1500 },
          }),
          ...(wrap ? [EditorView.lineWrapping] : []),
        ],
      });
      views.current = [editor];
    }
    let alive = true;
    void language(diff.path).then((extension) => {
      if (alive) {
        if (merge) {
          merge.a.dispatch({ effects: syntaxA.reconfigure(extension) });
          merge.b.dispatch({ effects: syntaxB.reconfigure(extension) });
        } else editor?.dispatch({ effects: syntaxB.reconfigure(extension) });
      }
    });
    return () => {
      alive = false;
      views.current = [];
      gutterCompartments.current = [];
      merge?.destroy();
      editor?.destroy();
    };
  }, [diff.contentHash, diff.oldText, diff.newText, diff.path, mode, wrap, collapse, readonly]);

  useEffect(() => {
    views.current.forEach((view, index) => {
      const compartment = gutterCompartments.current[index];
      if (compartment)
        view.dispatch({
          effects: compartment.reconfigure(
            makeGutter(mode === 'split' && index === 0 ? 'old' : 'new'),
          ),
        });
    });
  }, [selection, mode]);

  const navigate = (forward: boolean) => {
    const view = views.current.at(-1);
    if (view) (forward ? goToNextChunk : goToPreviousChunk)(view);
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === 'F7') {
        e.preventDefault();
        navigate(!e.shiftKey);
      }
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, []);

  if (diff.tooLarge)
    return (
      <div className="diff-container">
        <Empty
          icon={<FileWarning size={30} />}
          title="大文件预览已暂停"
          detail={`文件大小 ${(diff.size / 1024 / 1024).toFixed(1)} MB，按需加载以节省内存。`}
        />
        {diff.size <= 32 * 1024 * 1024 && (
          <button className="button center-button" onClick={onFull}>
            打开完整对比
          </button>
        )}
      </div>
    );
  if (diff.binary)
    return (
      <div className="diff-container">
        {diff.imageOld || diff.imageNew ? (
          <div className="image-comparison">
            <div>
              <span>原版本</span>
              {diff.imageOld ? <img src={diff.imageOld} alt="原版本" /> : <p>不存在</p>}
            </div>
            <div>
              <span>当前版本</span>
              {diff.imageNew ? <img src={diff.imageNew} alt="当前版本" /> : <p>不存在</p>}
            </div>
          </div>
        ) : (
          <Empty
            icon={<FileWarning size={28} />}
            title="此文件支持整体提交"
            detail="二进制、非 UTF-8 文件与子模块保持原始内容，不进行文本转换。"
          />
        )}
      </div>
    );
  return (
    <div className="diff-container">
      <div className="diff-toolbar">
        <div className="segmented">
          <IconButton title="双栏对比" active={mode === 'split'} onClick={() => setMode('split')}>
            <Columns2 size={15} />
          </IconButton>
          <IconButton
            title="单栏对比"
            active={mode === 'unified'}
            onClick={() => {
              setMode('unified');
              if (!readonly) setSelectPanel(true);
            }}
          >
            <Rows3 size={15} />
          </IconButton>
        </div>
        <span className="toolbar-divider" />
        <IconButton title="上一处差异 ⇧F7" onClick={() => navigate(false)}>
          <ArrowUp size={15} />
        </IconButton>
        <IconButton title="下一处差异 F7" onClick={() => navigate(true)}>
          <ArrowDown size={15} />
        </IconButton>
        <span className="muted small">{diff.hunks.length} 处改动</span>
        <span className="flex-spacer" />
        <IconButton title="折叠未修改区域" active={collapse} onClick={() => setCollapse((v) => !v)}>
          <ChevronsUpDown size={15} />
        </IconButton>
        <IconButton title="自动换行" active={wrap} onClick={() => setWrap((v) => !v)}>
          <WrapText size={15} />
        </IconButton>
        {!readonly && (
          <IconButton
            title="逐行选择与变更分组"
            active={selectPanel}
            onClick={() => setSelectPanel((v) => !v)}
          >
            <ListChecks size={15} />
          </IconButton>
        )}
      </div>
      <div className={`diff-labels ${mode}`}>
        {mode === 'split' && (
          <span>
            {readonly ? '基准版本' : '原版本'} {!readonly && <code>HEAD</code>}
          </span>
        )}
        <span>
          {readonly ? '所选版本' : '工作区'}
          {!readonly && <small>勾选行号旁的复选框加入提交</small>}
        </span>
      </div>
      <div className="diff-editor" ref={parent} />
      {selectPanel && !readonly && hunk && (
        <div className="hunk-panel">
          <div className="hunk-toolbar">
            <select
              aria-label="选择代码块"
              value={Math.min(hunkIndex, diff.hunks.length - 1)}
              onChange={(e) => setHunkIndex(Number(e.target.value))}
            >
              {diff.hunks.map((h, i) => (
                <option value={i} key={h.id}>
                  改动 {i + 1} · 第 {h.newStart} 行
                </option>
              ))}
            </select>
            <button className="text-button" onClick={() => onToggle?.(changedIds([hunk]))}>
              <Check size={13} />
              选择 / 取消此块
            </button>
            <span className="flex-spacer" />
            {groups && onGroupHunk && (
              <select
                aria-label="移动代码块到变更组"
                value={hunkGroups?.[hunk.id]?.groupId ?? ''}
                onChange={(e) => {
                  if (e.target.value) onGroupHunk(hunk.id, e.target.value);
                }}
              >
                <option value="">移动到变更组…</option>
                {groups.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.name}
                  </option>
                ))}
              </select>
            )}
          </div>
          <div className="hunk-lines">
            {hunk.lines.map((line) => (
              <label key={line.id} className={`hunk-line ${line.kind}`}>
                {line.kind === 'equal' ? (
                  <span className="checkbox-space" />
                ) : (
                  <input
                    type="checkbox"
                    checked={selected.has(line.id)}
                    onChange={() => onToggle?.([line.id])}
                  />
                )}
                <span className="line-number">{line.oldLine ?? line.newLine}</span>
                <span className="line-sign">
                  {line.kind === 'insert' ? '+' : line.kind === 'delete' ? '−' : ' '}
                </span>
                <code>{line.text.trimEnd() || ' '}</code>
              </label>
            ))}
          </div>
        </div>
      )}
      <div className="editor-footer">
        <span>UTF-8</span>
        <span>{diff.newText?.includes('\r\n') ? 'CRLF' : 'LF'}</span>
        <span className="flex-spacer" />
        {!readonly && (
          <span>
            {selection?.all
              ? '整个文件已选中'
              : selected.size
                ? `已选择 ${selected.size} 行改动`
                : '未加入提交'}
          </span>
        )}
      </div>
    </div>
  );
}
