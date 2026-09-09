import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, ChevronDown, Plus, Search } from 'lucide-react';
import { useVirtualizer } from '@tanstack/react-virtual';

export interface SelectOption {
  value: string;
  label: string;
  description?: string;
}

export default function SearchSelect({
  label,
  value,
  options,
  onChange,
  placeholder = '搜索并选择…',
  allowCreate = false,
  disabled = false,
}: {
  label: string;
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  placeholder?: string;
  allowCreate?: boolean;
  disabled?: boolean;
}) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [position, setPosition] = useState({ top: 0, left: 0, width: 0, height: 0 });
  const rows = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    const filtered = options.filter((option) =>
      (option.label + ' ' + (option.description ?? '')).toLocaleLowerCase().includes(needle),
    );
    const result = filtered.map((option) => ({ ...option, create: false }));
    if (allowCreate && query.trim() && !options.some((option) => option.value === query.trim())) {
      result.push({
        value: query.trim(),
        label: query.trim(),
        description: '新建远程分支',
        create: true,
      });
    }
    return result;
  }, [options, query, allowCreate]);
  const virtual = useVirtualizer({
    count: rows.length,
    getScrollElement: () => popup.current,
    estimateSize: () => 42,
    overscan: 6,
  });
  const selected = options.find((option) => option.value === value);
  const choose = (next: string) => {
    onChange(next);
    setOpen(false);
    setQuery('');
    input.current?.focus();
  };
  useEffect(() => {
    setActive(0);
  }, [query]);
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);
  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const rect = root.current?.getBoundingClientRect();
      if (!rect) return;
      const height = Math.min(252, Math.max(1, rows.length) * 42 + 8);
      const below = window.innerHeight - rect.bottom - 12;
      const above = rect.top - 12;
      const upwards = below < height && above > below;
      const available = Math.max(50, upwards ? above : below);
      const visible = Math.min(height, available);
      setPosition({
        left: rect.left,
        width: rect.width,
        height: visible,
        top: upwards ? rect.top - visible - 5 : rect.bottom + 5,
      });
    };
    place();
    window.addEventListener('resize', place);
    const scroll = (event: Event) => {
      if (!popup.current?.contains(event.target as Node)) place();
    };
    document.addEventListener('scroll', scroll, true);
    return () => {
      window.removeEventListener('resize', place);
      document.removeEventListener('scroll', scroll, true);
    };
  }, [open, rows.length]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: MouseEvent) => {
      if (
        root.current?.contains(event.target as Node) ||
        popup.current?.contains(event.target as Node)
      )
        return;
      if (allowCreate && query.trim()) onChange(query.trim());
      setOpen(false);
      setQuery('');
    };
    document.addEventListener('mousedown', outside);
    return () => document.removeEventListener('mousedown', outside);
  }, [open, query, allowCreate, onChange]);
  const move = (index: number) => {
    const next = Math.max(0, Math.min(rows.length - 1, index));
    setActive(next);
    virtual.scrollToIndex(next);
  };

  return (
    <div className="search-select-field">
      <label htmlFor={id}>{label}</label>
      <div ref={root} className={`search-select-control ${open ? 'open' : ''}`}>
        {open ? <Search size={14} /> : null}
        <input
          ref={input}
          id={id}
          role="combobox"
          autoComplete="off"
          spellCheck={false}
          aria-expanded={open}
          aria-controls={id + '-list'}
          aria-autocomplete="list"
          aria-activedescendant={open && rows[active] ? id + '-option-' + active : undefined}
          placeholder={placeholder}
          disabled={disabled}
          value={open ? query : (selected?.label ?? value)}
          onClick={() => {
            if (!open) {
              setQuery('');
              setActive(0);
              setOpen(true);
            }
          }}
          onChange={(event) => {
            setQuery(event.target.value);
            setOpen(true);
          }}
          onBlur={(event) => {
            if (
              root.current?.contains(event.relatedTarget as Node) ||
              popup.current?.contains(event.relatedTarget as Node)
            )
              return;
            if (allowCreate && query.trim()) onChange(query.trim());
            setOpen(false);
            setQuery('');
          }}
          onKeyDown={(event) => {
            if (event.key === 'Escape' && open) {
              event.preventDefault();
              event.stopPropagation();
              setOpen(false);
              setQuery('');
            } else if (['ArrowDown', 'ArrowUp'].includes(event.key)) {
              event.preventDefault();
              if (!open) {
                setOpen(true);
                setQuery('');
                setActive(0);
              } else move(active + (event.key === 'ArrowDown' ? 1 : -1));
            } else if (open && ['Home', 'End'].includes(event.key)) {
              event.preventDefault();
              move(event.key === 'Home' ? 0 : rows.length - 1);
            } else if (event.key === 'Enter') {
              event.preventDefault();
              if (open && rows[active]) choose(rows[active].value);
              else {
                setOpen(true);
                setQuery('');
                setActive(0);
              }
            }
          }}
        />
        <button
          type="button"
          disabled={disabled}
          tabIndex={-1}
          aria-label={'展开' + label}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => {
            input.current?.focus();
            setQuery('');
            setActive(0);
            setOpen(!open);
          }}
        >
          <ChevronDown size={14} />
        </button>
      </div>
      {open &&
        createPortal(
          <div
            ref={popup}
            id={id + '-list'}
            role="listbox"
            aria-label={label}
            className="search-select-popup"
            style={position}
          >
            {rows.length ? (
              <div style={{ height: virtual.getTotalSize(), position: 'relative' }}>
                {virtual.getVirtualItems().map((row) => {
                  const option = rows[row.index];
                  return (
                    <div
                      key={option.value}
                      id={id + '-option-' + row.index}
                      role="option"
                      aria-selected={option.value === value}
                      className={`search-select-option ${row.index === active ? 'active' : ''}`}
                      style={{ height: row.size, transform: `translateY(${row.start}px)` }}
                      onMouseDown={(event) => event.preventDefault()}
                      onMouseMove={() => setActive(row.index)}
                      onClick={() => choose(option.value)}
                    >
                      {option.create ? (
                        <Plus size={14} />
                      ) : option.value === value ? (
                        <Check size={14} />
                      ) : (
                        <span className="select-check-space" />
                      )}
                      <span>
                        <strong>{option.label}</strong>
                        {option.description && <small>{option.description}</small>}
                      </span>
                    </div>
                  );
                })}
              </div>
            ) : (
              <div className="search-select-empty">没有匹配的选项</div>
            )}
          </div>,
          document.body,
        )}
    </div>
  );
}
