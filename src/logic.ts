import type { Commit, DiffHunk, FileDiff, Selection, RepoGroups } from './types.js';

export function fileRowRange(visible: string[], anchor: string | null, target: string): string[] {
  const end = visible.indexOf(target);
  if (end < 0) return [];
  const start = anchor ? visible.indexOf(anchor) : -1;
  return start < 0 ? [target] : visible.slice(Math.min(start, end), Math.max(start, end) + 1);
}

export function selectFileRows(
  visible: string[],
  selected: Set<string>,
  anchor: string | null,
  target: string,
  gesture: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean },
): { selected: Set<string>; anchor: string } {
  const additive = gesture.metaKey || gesture.ctrlKey;
  const visibleSet = new Set(visible);
  const next = new Set(additive ? [...selected].filter((key) => visibleSet.has(key)) : []);
  if (gesture.shiftKey) {
    fileRowRange(visible, anchor, target).forEach((key) => next.add(key));
    return { selected: next, anchor: anchor && visibleSet.has(anchor) ? anchor : target };
  }
  if (additive && next.has(target)) next.delete(target);
  else if (visibleSet.has(target)) next.add(target);
  return { selected: next, anchor: target };
}

export function setDiffLines(
  diff: FileDiff,
  current: Selection | undefined,
  ids: string[],
  include: boolean,
): Selection | undefined {
  if (include && current?.all) return current;
  const chosen = new Set(current?.all ? changedIds(diff.hunks) : (current?.lineIds ?? []));
  ids.forEach((id) => (include ? chosen.add(id) : chosen.delete(id)));
  if (!chosen.size) return undefined;
  return { fileId: diff.fileId, all: false, lineIds: [...chosen], contentHash: diff.contentHash };
}

export interface AiDraftContext {
  repoId?: string;
  snapshot?: string;
  selection: string;
  draft: string;
}

export function selectionKey(selections: Record<string, Selection>): string {
  return JSON.stringify(
    Object.values(selections)
      .map((s) => ({
        fileId: s.fileId,
        all: s.all,
        contentHash: s.contentHash ?? null,
        lineIds: [...s.lineIds].sort(),
      }))
      .sort((a, b) => a.fileId.localeCompare(b.fileId)),
  );
}

export function aiDraftDecision(
  before: AiDraftContext,
  current: AiDraftContext,
): 'apply' | 'preview' | 'discard' {
  if (
    before.repoId !== current.repoId ||
    before.snapshot !== current.snapshot ||
    before.selection !== current.selection
  )
    return 'discard';
  return before.draft === current.draft ? 'apply' : 'preview';
}

export const changedIds = (hunks: DiffHunk[]) =>
  hunks.flatMap((hunk) =>
    hunk.lines.filter((line) => line.kind !== 'equal').map((line) => line.id),
  );
export const defaultGroups = (): RepoGroups => ({
  groups: [{ id: 'default', name: '默认变更' }],
  fileGroups: {},
  hunkGroups: {},
});

export function pruneCommittedGroups(groups: RepoGroups, selections: Selection[]): RepoGroups {
  const fileGroups = { ...groups.fileGroups };
  const hunkGroups = { ...groups.hunkGroups };
  for (const selection of selections) {
    if (selection.all) {
      delete fileGroups[selection.fileId];
      delete hunkGroups[selection.fileId];
      continue;
    }
    const selected = new Set(selection.lineIds);
    const assignments = hunkGroups[selection.fileId];
    if (!assignments) continue;
    hunkGroups[selection.fileId] = Object.fromEntries(
      Object.entries(assignments).filter(
        ([, assignment]) => !assignment.lineIds.every((id) => selected.has(id)),
      ),
    );
    if (!Object.keys(hunkGroups[selection.fileId]).length) delete hunkGroups[selection.fileId];
  }
  return { ...groups, fileGroups, hunkGroups };
}

export function refreshGroupAnchors(groups: RepoGroups, diff: FileDiff): RepoGroups {
  const assignments = groups.hunkGroups[diff.fileId];
  if (!assignments) return groups;
  let changed = false;
  const updated = Object.fromEntries(
    Object.entries(assignments).map(([id, assignment]) => {
      const hunk = diff.hunks.find((h) => h.id === id);
      if (!hunk) {
        if (assignment.stale) return [id, assignment];
        changed = true;
        return [id, { ...assignment, stale: true }];
      }
      const ids = changedIds([hunk]);
      if (
        !assignment.stale &&
        assignment.contentHash === diff.contentHash &&
        ids.length === assignment.lineIds.length &&
        ids.every((value, index) => value === assignment.lineIds[index])
      )
        return [id, assignment];
      changed = true;
      return [id, { ...assignment, lineIds: ids, contentHash: diff.contentHash, stale: false }];
    }),
  );
  return changed
    ? { ...groups, hunkGroups: { ...groups.hunkGroups, [diff.fileId]: updated } }
    : groups;
}

export function toggleLines(
  diff: FileDiff,
  current: Selection | undefined,
  ids: string[],
): Selection | undefined {
  const chosen = new Set(current?.all ? changedIds(diff.hunks) : (current?.lineIds ?? []));
  const remove = ids.every((id) => chosen.has(id));
  ids.forEach((id) => (remove ? chosen.delete(id) : chosen.add(id)));
  if (!chosen.size) return undefined;
  return { fileId: diff.fileId, all: false, lineIds: [...chosen], contentHash: diff.contentHash };
}

export interface GraphRow {
  lane: number;
  color: number;
  before: string[];
  after: string[];
  parentLanes: number[];
  continues: number[];
}
export function buildGraph(commits: Commit[]): GraphRow[] {
  let lanes: string[] = [];
  const colors = new Map<string, number>();
  let nextColor = 0;
  return commits.map((commit) => {
    let lane = lanes.indexOf(commit.oid);
    if (lane < 0) {
      lane = lanes.length;
      lanes.push(commit.oid);
    }
    if (!colors.has(commit.oid)) colors.set(commit.oid, nextColor++ % 7);
    const before = [...lanes];
    const color = colors.get(commit.oid)!;
    const next = [...lanes];
    next.splice(lane, 1);
    commit.parents.forEach((parent, index) => {
      if (!next.includes(parent)) next.splice(Math.min(lane + index, next.length), 0, parent);
      if (!colors.has(parent)) colors.set(parent, index === 0 ? color : nextColor++ % 7);
    });
    const after = [...next];
    const parentLanes = commit.parents.map((parent) => next.indexOf(parent));
    const continues = before.map((oid, index) => (index === lane ? -1 : next.indexOf(oid)));
    lanes = next;
    return { lane, color, before, after, parentLanes, continues };
  });
}

export interface MergeBlock {
  start: number;
  end: number;
  ours: string;
  base: string;
  theirs: string;
  marker: string;
}
export function parseConflicts(text: string): MergeBlock[] {
  const lines = text.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const result: MergeBlock[] = [];
  let offset = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const match = line.match(/^(<{7,})(?:[ \r\n]|$)/);
    if (!match) {
      offset += line.length;
      continue;
    }
    const markerSize = match[1].length;
    const start = offset;
    let ours = '',
      base = '',
      theirs = '';
    let section: 'ours' | 'base' | 'theirs' = 'ours';
    let end = -1;
    let length = line.length;
    for (let j = i + 1; j < lines.length; j++) {
      const current = lines[j];
      length += current.length;
      if (current.startsWith('|'.repeat(markerSize)) && section === 'ours') {
        section = 'base';
        continue;
      }
      if (current.trimEnd() === '='.repeat(markerSize)) {
        section = 'theirs';
        continue;
      }
      if (current.startsWith('>'.repeat(markerSize)) && section === 'theirs') {
        end = j;
        break;
      }
      if (section === 'ours') ours += current;
      else if (section === 'base') base += current;
      else theirs += current;
    }
    if (end < 0) {
      offset += line.length;
      continue;
    }
    result.push({
      start,
      end: start + length,
      ours,
      base,
      theirs,
      marker: text.slice(start, start + length),
    });
    offset += length;
    i = end;
  }
  return result;
}

export function acceptConflict(
  text: string,
  block: MergeBlock,
  side: 'ours' | 'theirs' | 'both',
): string {
  if (text.slice(block.start, block.end) !== block.marker)
    throw new Error('冲突区块已经改变，请重新选择。');
  const value =
    side === 'ours' ? block.ours : side === 'theirs' ? block.theirs : block.ours + block.theirs;
  return text.slice(0, block.start) + value + text.slice(block.end);
}

export interface TodoLine {
  id: number;
  raw: string;
  action: string;
  hash: string;
  message: string;
  editable: boolean;
}
const editActions = new Set([
  'pick',
  'p',
  'reword',
  'r',
  'edit',
  'e',
  'squash',
  's',
  'fixup',
  'f',
  'drop',
  'd',
]);
const actionNames: Record<string, string> = {
  p: 'pick',
  r: 'reword',
  e: 'edit',
  s: 'squash',
  f: 'fixup',
  d: 'drop',
};
export function parseTodo(text: string): TodoLine[] {
  return text.split('\n').map((raw, id) => {
    const match = raw.match(/^(\S+)\s+(\S+)(?:\s+(.*))?$/);
    const editable = !!match && editActions.has(match[1]) && /^[a-f0-9]{7,64}$/.test(match[2]);
    const action = match?.[1] ?? '';
    return {
      id,
      raw,
      action: actionNames[action] ?? action,
      hash: match?.[2] ?? '',
      message: match?.[3] ?? '',
      editable,
    };
  });
}
export function serializeTodo(lines: TodoLine[]): string {
  return lines
    .map((line) =>
      line.editable
        ? `${line.action} ${line.hash}${line.message ? ' ' + line.message : ''}`
        : line.raw,
    )
    .join('\n');
}
export function moveTodo(lines: TodoLine[], index: number, delta: number): TodoLine[] {
  const target = index + delta;
  if (!lines[index]?.editable || !lines[target]?.editable) return lines;
  const result = [...lines];
  [result[index], result[target]] = [result[target], result[index]];
  return result;
}

export function formatTime(timestamp: number): string {
  return new Intl.DateTimeFormat('zh-CN', {
    timeZone: 'Asia/Shanghai',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(timestamp * 1000);
}
export const shortOid = (oid: string) => oid.slice(0, 8);
export function basename(path: string) {
  return path.split('/').at(-1) ?? path;
}
export function dirname(path: string) {
  const at = path.lastIndexOf('/');
  return at < 0 ? '' : path.slice(0, at);
}
export function fileStatus(status: string): string {
  if (status.includes('U') || status === 'AA' || status === 'DD') return '冲突';
  if (status.includes('R')) return '重命名';
  if (status.includes('D')) return '删除';
  if (status.includes('A') || status.includes('?')) return '新增';
  return '修改';
}
