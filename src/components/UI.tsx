import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { ArrowDown, ArrowUp, Check, ChevronDown, LoaderCircle, X } from 'lucide-react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { api, errorMessage } from '../api';
import { moveTodo, parseTodo, serializeTodo } from '../logic';
import type { GitPrompt } from '../types';

export function IconButton({
  title,
  children,
  onClick,
  active,
  disabled,
  className = '',
}: {
  title: string;
  children: ReactNode;
  onClick?: () => void;
  active?: boolean;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <button
      className={`icon-button ${active ? 'active' : ''} ${className}`}
      title={title}
      aria-label={title}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}
export function Spinner({ text = '正在读取…' }: { text?: string }) {
  return (
    <div className="loading-state">
      <LoaderCircle size={18} className="spin" />
      <span>{text}</span>
    </div>
  );
}
export function Empty({
  icon,
  title,
  detail,
}: {
  icon?: ReactNode;
  title: string;
  detail?: string;
}) {
  return (
    <div className="empty-state">
      {icon && <div className="empty-icon">{icon}</div>}
      <h3>{title}</h3>
      {detail && <p>{detail}</p>}
    </div>
  );
}

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
  action: () => void;
}
export function Menu({
  items,
  x,
  y,
  onClose,
}: {
  items: MenuItem[];
  x: number;
  y: number;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const close = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', close);
    document.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('mousedown', close);
      document.removeEventListener('keydown', key);
    };
  }, [onClose]);
  return createPortal(
    <div
      ref={ref}
      className="context-menu"
      role="menu"
      style={{
        left: Math.min(x, window.innerWidth - 278),
        top: Math.min(y, Math.max(8, window.innerHeight - items.length * 32 - 16)),
      }}
    >
      {items.map((item, i) => (
        <div key={`${item.label}-${i}`}>
          {item.separator && <div className="menu-separator" />}
          <button
            role="menuitem"
            disabled={item.disabled}
            className={item.danger ? 'danger-text' : ''}
            onClick={() => {
              onClose();
              item.action();
            }}
          >
            {item.icon ?? <span className="menu-icon-space" />}
            <span>{item.label}</span>
            {item.shortcut && <kbd>{item.shortcut}</kbd>}
          </button>
        </div>
      ))}
    </div>,
    document.body,
  );
}

export function Dropdown({
  label,
  items,
  icon,
}: {
  label: string;
  items: MenuItem[];
  icon?: ReactNode;
}) {
  const [position, setPosition] = useState<{ x: number; y: number } | null>(null);
  return (
    <>
      <button
        className="dropdown-button"
        onClick={(event) => {
          const rect = event.currentTarget.getBoundingClientRect();
          setPosition(position ? null : { x: rect.left, y: rect.bottom + 5 });
        }}
      >
        {icon}
        <span>{label}</span>
        <ChevronDown size={12} />
      </button>
      {position && <Menu {...position} items={items} onClose={() => setPosition(null)} />}
    </>
  );
}

export interface Field {
  key: string;
  label: string;
  value?: string;
  placeholder?: string;
  type?: 'text' | 'textarea' | 'select' | 'checkbox' | 'password';
  options?: { value: string; label: string }[];
  required?: boolean;
}
export interface DialogSpec {
  title: string;
  description?: string;
  fields?: Field[];
  confirmLabel?: string;
  danger?: boolean;
  submit: (values: Record<string, string>) => Promise<void> | void;
}
export function Dialog({ spec, onClose }: { spec: DialogSpec; onClose: () => void }) {
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(
      (spec.fields ?? []).map((field) => [
        field.key,
        field.value ?? (field.type === 'checkbox' ? 'false' : ''),
      ]),
    ),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  return (
    <Modal title={spec.title} onClose={busy ? undefined : onClose}>
      <form
        onSubmit={async (event) => {
          event.preventDefault();
          setError('');
          setBusy(true);
          try {
            await spec.submit(values);
            onClose();
          } catch (error) {
            setError(errorMessage(error));
          } finally {
            setBusy(false);
          }
        }}
      >
        {spec.description && <p className="dialog-description">{spec.description}</p>}
        {spec.fields?.map((field) => (
          <label
            className={`form-field ${field.type === 'checkbox' ? 'checkbox-field' : ''}`}
            key={field.key}
          >
            {field.type === 'checkbox' ? (
              <>
                <input
                  type="checkbox"
                  checked={values[field.key] === 'true'}
                  onChange={(event) =>
                    setValues((v) => ({ ...v, [field.key]: String(event.target.checked) }))
                  }
                />
                <span>{field.label}</span>
              </>
            ) : (
              <>
                <span>{field.label}</span>
                {field.type === 'textarea' ? (
                  <textarea
                    autoFocus={field === spec.fields?.[0]}
                    value={values[field.key]}
                    required={field.required}
                    placeholder={field.placeholder}
                    onChange={(event) =>
                      setValues((v) => ({ ...v, [field.key]: event.target.value }))
                    }
                    rows={5}
                  />
                ) : field.type === 'select' ? (
                  <select
                    value={values[field.key]}
                    required={field.required}
                    onChange={(event) =>
                      setValues((v) => ({ ...v, [field.key]: event.target.value }))
                    }
                  >
                    {field.options?.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    autoFocus={field === spec.fields?.[0]}
                    type={field.type ?? 'text'}
                    value={values[field.key]}
                    required={field.required}
                    placeholder={field.placeholder}
                    onChange={(event) =>
                      setValues((v) => ({ ...v, [field.key]: event.target.value }))
                    }
                  />
                )}
              </>
            )}
          </label>
        ))}
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className={`button ${spec.danger ? 'danger' : 'primary'}`} disabled={busy}>
            {busy && <LoaderCircle size={14} className="spin" />}
            {spec.confirmLabel ?? '确定'}
          </button>
        </div>
      </form>
    </Modal>
  );
}

export function Modal({
  title,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  children: ReactNode;
  onClose?: () => void;
  wide?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const key = (event: KeyboardEvent) => {
      const owner =
        event.target instanceof Element ? event.target.closest('[role="dialog"]') : null;
      if (owner && owner !== ref.current) return;
      if (event.key === 'Escape' && closeRef.current) {
        event.preventDefault();
        closeRef.current();
      }
      if (event.key === 'Tab') {
        const focusable = ref.current?.querySelectorAll<HTMLElement>(
          'button:not(:disabled):not([tabindex="-1"]), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex="0"]',
        );
        if (!focusable?.length) return;
        const first = focusable[0],
          last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        }
        if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };
    document.addEventListener('keydown', key);
    (
      ref.current?.querySelector<HTMLElement>('input, textarea, select') ??
      ref.current?.querySelector<HTMLElement>('button')
    )?.focus();
    return () => {
      document.removeEventListener('keydown', key);
      previous?.focus();
    };
  }, []);
  return createPortal(
    <div className="modal-backdrop">
      <div
        className={`modal ${wide ? 'wide' : ''}`}
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="modal-header">
          <h2>{title}</h2>
          {onClose && (
            <IconButton title="关闭" onClick={onClose}>
              <X size={17} />
            </IconButton>
          )}
        </div>
        {children}
      </div>
    </div>,
    document.body,
  );
}

export function PromptDialog({ prompt, onClose }: { prompt: GitPrompt; onClose: () => void }) {
  const [content, setContent] = useState(prompt.content);
  const [todo, setTodo] = useState(() => parseTodo(prompt.content));
  const [secret, setSecret] = useState('');
  const [error, setError] = useState('');
  const submit = async (value: string | null) => {
    try {
      await api.answer(prompt.id, value);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    }
  };
  return (
    <Modal title={prompt.title} wide={prompt.kind === 'sequence'} onClose={() => void submit(null)}>
      {prompt.kind === 'sequence' ? (
        <>
          <p className="dialog-description">
            从上到下应用提交。通过右侧箭头调整顺序；合并拓扑指令保留在原位。
          </p>
          <div className="todo-list">
            {todo.map((line, index) =>
              !line.raw.trim() || line.raw.startsWith('#') ? null : (
                <div key={line.id} className={`todo-row ${line.editable ? '' : 'readonly'}`}>
                  {line.editable ? (
                    <select
                      aria-label="提交操作"
                      value={line.action}
                      onChange={(event) =>
                        setTodo((lines) =>
                          lines.map((v, i) =>
                            i === index ? { ...v, action: event.target.value } : v,
                          ),
                        )
                      }
                    >
                      <option value="pick">保留</option>
                      <option value="reword">改说明</option>
                      <option value="edit">暂停编辑</option>
                      <option value="squash">合并并编辑</option>
                      <option value="fixup">压缩</option>
                      <option value="drop">删除</option>
                    </select>
                  ) : (
                    <span className="todo-control">{line.action}</span>
                  )}
                  <code>{line.hash.slice(0, 8)}</code>
                  <span className="truncate">{line.message || line.raw}</span>
                  {line.editable && (
                    <>
                      <IconButton
                        title="上移"
                        disabled={!todo[index - 1]?.editable}
                        onClick={() => setTodo((v) => moveTodo(v, index, -1))}
                      >
                        <ArrowUp size={14} />
                      </IconButton>
                      <IconButton
                        title="下移"
                        disabled={!todo[index + 1]?.editable}
                        onClick={() => setTodo((v) => moveTodo(v, index, 1))}
                      >
                        <ArrowDown size={14} />
                      </IconButton>
                    </>
                  )}
                </div>
              ),
            )}
          </div>
        </>
      ) : prompt.kind === 'askpass' ? (
        <label className="form-field">
          <span>{prompt.content}</span>
          <input
            autoFocus
            type={/username|用户名/i.test(prompt.content) ? 'text' : 'password'}
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void submit(secret);
            }}
            autoComplete="off"
          />
        </label>
      ) : (
        <textarea
          className="message-editor"
          value={content}
          onChange={(e) => setContent(e.target.value)}
          autoFocus
          spellCheck={false}
        />
      )}
      {error && <div className="inline-error">{error}</div>}
      <div className="dialog-actions">
        <button className="button" onClick={() => void submit(null)}>
          取消操作
        </button>
        <button
          className="button primary"
          disabled={prompt.kind === 'editor' && !content.trim()}
          onClick={() =>
            void submit(
              prompt.kind === 'sequence'
                ? serializeTodo(todo)
                : prompt.kind === 'askpass'
                  ? secret
                  : content,
            )
          }
        >
          <Check size={14} />
          {prompt.kind === 'sequence' ? '应用提交序列' : '继续'}
        </button>
      </div>
    </Modal>
  );
}

export function VirtualList<T>({
  items,
  rowHeight,
  render,
  className = '',
  scrollToIndex,
}: {
  items: T[];
  rowHeight: number;
  render: (item: T, index: number) => ReactNode;
  className?: string;
  scrollToIndex?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: items.length,
    getScrollElement: () => ref.current,
    estimateSize: () => rowHeight,
    overscan: 10,
  });
  useEffect(() => {
    if (scrollToIndex !== undefined && items.length)
      virtual.scrollToIndex(scrollToIndex, { align: 'auto' });
  }, [scrollToIndex, items.length, virtual]);
  return (
    <div className={`virtual-scroll ${className}`} ref={ref}>
      <div style={{ height: virtual.getTotalSize(), width: '100%', position: 'relative' }}>
        {virtual.getVirtualItems().map((row) => (
          <div
            key={row.key}
            style={{
              position: 'absolute',
              width: '100%',
              height: row.size,
              transform: `translateY(${row.start}px)`,
            }}
          >
            {render(items[row.index], row.index)}
          </div>
        ))}
      </div>
    </div>
  );
}

export function ResizeHandle({
  value,
  onChange,
  direction = 'horizontal',
  min = 220,
  max = 540,
}: {
  value: number;
  onChange: (size: number) => void;
  direction?: 'horizontal' | 'vertical';
  min?: number;
  max?: number;
}) {
  const dragging = useRef(false);
  return (
    <div
      role="separator"
      aria-label="调整面板大小"
      aria-orientation={direction === 'horizontal' ? 'vertical' : 'horizontal'}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      tabIndex={0}
      className={`resize-handle ${direction}`}
      onKeyDown={(e) => {
        if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') onChange(Math.max(min, value - 10));
        if (e.key === 'ArrowRight' || e.key === 'ArrowDown') onChange(Math.min(max, value + 10));
      }}
      onPointerDown={(event) => {
        event.preventDefault();
        event.currentTarget.focus();
        dragging.current = true;
        event.currentTarget.setPointerCapture(event.pointerId);
        const start = direction === 'horizontal' ? event.clientX : event.clientY;
        const initial = value;
        const target = event.currentTarget;
        const move = (e: PointerEvent) => {
          if (dragging.current)
            onChange(
              Math.max(
                min,
                Math.min(
                  max,
                  initial + (direction === 'horizontal' ? e.clientX : e.clientY) - start,
                ),
              ),
            );
        };
        const stop = () => {
          dragging.current = false;
          target.removeEventListener('pointermove', move);
          target.removeEventListener('pointerup', stop);
          target.removeEventListener('pointercancel', stop);
        };
        target.addEventListener('pointermove', move);
        target.addEventListener('pointerup', stop);
        target.addEventListener('pointercancel', stop);
      }}
    />
  );
}
