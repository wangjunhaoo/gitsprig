import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  acceptConflict,
  buildGraph,
  moveTodo,
  parseConflicts,
  parseTodo,
  serializeTodo,
  toggleLines,
  pruneCommittedGroups,
  refreshGroupAnchors,
  aiDraftDecision,
  selectionKey,
  fileRowRange,
  selectFileRows,
  setDiffLines,
} from './logic.js';
import type { Commit, FileDiff, RepoGroups } from './types.js';

describe('文件行多选', () => {
  const visible = ['a:default', 'b:default', 'c:task', 'a:task'];
  it('Shift 正反向连续选择，并固定起点以便缩小范围', () => {
    const first = selectFileRows(visible, new Set(), null, visible[0], {});
    const range = selectFileRows(visible, first.selected, first.anchor, visible[2], {
      shiftKey: true,
    });
    assert.deepEqual([...range.selected], visible.slice(0, 3));
    assert.deepEqual(
      [
        ...selectFileRows(visible, range.selected, range.anchor, visible[1], { shiftKey: true })
          .selected,
      ],
      visible.slice(0, 2),
    );
    assert.deepEqual(fileRowRange(visible, visible[2], visible[0]), visible.slice(0, 3));
  });
  it('Command/Ctrl 增减单项，组合 Shift 时追加范围', () => {
    const current = new Set([visible[0], visible[1]]);
    assert.deepEqual(
      [...selectFileRows(visible, current, visible[0], visible[1], { metaKey: true }).selected],
      [visible[0]],
    );
    assert.deepEqual(
      [
        ...selectFileRows(visible, current, visible[1], visible[3], {
          ctrlKey: true,
          shiftKey: true,
        }).selected,
      ],
      visible,
    );
  });
  it('筛选或折叠后的范围不包含隐藏行，同一文件的不同分组保留独立行身份', () => {
    assert.deepEqual(fileRowRange([visible[2], visible[3]], visible[0], visible[3]), [visible[3]]);
    assert.deepEqual(fileRowRange(visible, visible[0], visible[3]), visible);
    assert.deepEqual(fileRowRange(visible, visible[0], 'missing'), []);
  });
});

describe('批量勾选代码块', () => {
  const diff = {
    fileId: 'f',
    contentHash: 'current',
    hunks: [
      {
        id: 'h',
        oldStart: 1,
        newStart: 1,
        lines: ['first', 'second', 'third'].map((id) => ({
          id,
          kind: 'insert',
          text: id,
          oldLine: null,
          newLine: 1,
        })),
      },
    ],
  } as FileDiff;
  it('勾选与取消一组时保留其他组的选择', () => {
    const current = { fileId: 'f', all: false, lineIds: ['first'], contentHash: 'current' };
    const added = setDiffLines(diff, current, ['second'], true);
    assert.deepEqual(added?.lineIds, ['first', 'second']);
    assert.deepEqual(setDiffLines(diff, added, ['second'], false)?.lineIds, ['first']);
    assert.equal(setDiffLines(diff, current, ['first'], false), undefined);
  });
  it('重复勾选不会把整文件提交改为部分提交', () => {
    const whole = { fileId: 'f', all: true, lineIds: [] };
    assert.equal(setDiffLines(diff, whole, ['first'], true), whole);
    assert.deepEqual(setDiffLines(diff, whole, ['first'], false)?.lineIds, ['second', 'third']);
  });
});

describe('AI 提交草稿保护', () => {
  const before = { repoId: 'repo', snapshot: 'snapshot', selection: 'selected', draft: '原草稿' };
  it('只有相同仓库、快照、选择和草稿才直接填入', () => {
    assert.equal(aiDraftDecision(before, { ...before }), 'apply');
    assert.equal(aiDraftDecision(before, { ...before, draft: '手工编辑' }), 'preview');
    for (const key of ['repoId', 'snapshot', 'selection']) {
      assert.equal(aiDraftDecision(before, { ...before, [key]: 'changed' }), 'discard');
    }
  });
  it('选择顺序不影响身份，内容哈希与选择范围改变身份', () => {
    const a = { fileId: 'a', all: false, lineIds: ['x', 'y'], contentHash: 'hash' };
    const b = { fileId: 'b', all: true, lineIds: [] };
    assert.equal(selectionKey({ a, b }), selectionKey({ b, a: { ...a, lineIds: ['y', 'x'] } }));
    assert.notEqual(selectionKey({ a }), selectionKey({ a: { ...a, contentHash: 'new' } }));
    assert.notEqual(selectionKey({ a }), selectionKey({ a: { ...a, lineIds: ['x'] } }));
  });
});

describe('三方冲突文本', () => {
  it('准确解析 diff3、多处冲突及 CRLF', () => {
    const text =
      'start\r\n<<<<<<< HEAD\r\nours\r\n||||||| base\r\nbase\r\n=======\r\ntheirs\r\n>>>>>>> topic\r\nend\r\n';
    const blocks = parseConflicts(text);
    assert.equal(blocks.length, 1);
    assert.equal(blocks[0].base, 'base\r\n');
    assert.equal(acceptConflict(text, blocks[0], 'both'), 'start\r\nours\r\ntheirs\r\nend\r\n');
  });
  it('不修改已过期的冲突区间', () => {
    const text = '<<<<<<< A\na\n=======\nb\n>>>>>>> B\n';
    assert.throws(() => acceptConflict('prefix\n' + text, parseConflicts(text)[0], 'ours'));
  });
  it('保留非冲突上下文和其他冲突', () => {
    const text =
      '<<<<<<< A\na\n=======\nb\n>>>>>>> B\nmiddle\n<<<<<<< A\nc\n=======\nd\n>>>>>>> B\n';
    const updated = acceptConflict(text, parseConflicts(text)[0], 'theirs');
    assert.ok(updated.startsWith('b\nmiddle\n'));
    assert.equal(parseConflicts(updated).length, 1);
  });
});

describe('变基序列', () => {
  it('只允许相邻普通提交重排，保留合并控制指令', () => {
    const text =
      'label onto\npick aaaaaaaa one\npick bbbbbbbb two\nreset onto\nmerge -C cccccccc topic\n';
    const parsed = parseTodo(text);
    assert.ok(
      serializeTodo(moveTodo(parsed, 1, 1)).includes('pick bbbbbbbb two\npick aaaaaaaa one'),
    );
    assert.equal(moveTodo(parsed, 2, 1), parsed);
    assert.equal(serializeTodo(parsed), text);
  });
});

describe('提交图', () => {
  const commit = (oid: string, parents: string[]): Commit => ({
    oid,
    parents,
    author: 'a',
    email: 'a@invalid',
    timestamp: 1,
    subject: oid,
    decorations: '',
  });
  it('合并分支在父提交处重新汇合', () => {
    const graph = buildGraph([
      commit('m', ['a', 'b']),
      commit('a', ['c']),
      commit('b', ['c']),
      commit('c', []),
    ]);
    assert.equal(graph[0].parentLanes.length, 2);
    assert.deepEqual(graph[2].after, ['c']);
    assert.deepEqual(graph[3].after, []);
  });
});

describe('部分选择', () => {
  it('取消整文件中的一行后，仅保留其他行', () => {
    const diff = {
      fileId: 'f',
      contentHash: 'h',
      hunks: [
        {
          id: 'x',
          oldStart: 1,
          newStart: 1,
          lines: [
            { id: 'd', kind: 'delete', text: 'old', oldLine: 1, newLine: null },
            { id: 'i', kind: 'insert', text: 'new', oldLine: null, newLine: 1 },
          ],
        },
      ],
    } as FileDiff;
    assert.deepEqual(toggleLines(diff, { fileId: 'f', all: true, lineIds: [] }, ['i'])?.lineIds, [
      'd',
    ]);
    assert.equal(toggleLines(diff, { fileId: 'f', all: false, lineIds: ['i'] }, ['i']), undefined);
  });
});

describe('任务分组', () => {
  const groups = (): RepoGroups => ({
    groups: [
      { id: 'default', name: '默认' },
      { id: 'task', name: '任务' },
    ],
    fileGroups: {},
    hunkGroups: {
      file: {
        first: { groupId: 'default', contentHash: 'before', lineIds: ['first-line'] },
        second: { groupId: 'task', contentHash: 'before', lineIds: ['second-old-line'] },
      },
    },
  });
  it('提交一个代码块后只清理已提交分组', () => {
    const next = pruneCommittedGroups(groups(), [
      { fileId: 'file', all: false, lineIds: ['first-line'] },
    ]);
    assert.equal(next.hunkGroups.file.first, undefined);
    assert.equal(next.hunkGroups.file.second.groupId, 'task');
  });
  it('行号移动后刷新选择锚点，无法对应的块标记待归组', () => {
    const diff = {
      fileId: 'file',
      contentHash: 'after',
      hunks: [
        {
          id: 'second',
          oldStart: 20,
          newStart: 21,
          lines: [
            { id: 'second-new-line', kind: 'insert', text: 'new', oldLine: null, newLine: 21 },
          ],
        },
      ],
    } as FileDiff;
    const next = refreshGroupAnchors(groups(), diff);
    assert.equal(next.hunkGroups.file.first.stale, true);
    assert.deepEqual(next.hunkGroups.file.second.lineIds, ['second-new-line']);
    assert.equal(next.hunkGroups.file.second.stale, false);
  });
});
