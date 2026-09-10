import { useMemo } from 'react';
import { VirtualList } from './UI';
import { shortOid } from '../logic';

export default function Blame({ text }: { text: string }) {
  const rows = useMemo(() => {
    const result: { oid: string; author: string; line: number; content: string }[] = [];
    let oid = '',
      author = '',
      line = 0;
    for (const record of text.split('\n')) {
      const header = record.match(/^([0-9a-f]{40,64}) \d+ (\d+)(?: \d+)?$/);
      if (header) {
        oid = header[1];
        line = Number(header[2]);
      } else if (record.startsWith('author ')) author = record.slice(7);
      else if (record.startsWith('\t'))
        result.push({ oid, author, line, content: record.slice(1) });
    }
    return result;
  }, [text]);
  return (
    <div className="blame-view">
      <div className="blame-heading">
        <span>提交</span>
        <span>作者</span>
        <span>行</span>
        <span>内容</span>
      </div>
      <VirtualList
        items={rows}
        rowHeight={25}
        render={(row) => (
          <div className="blame-row">
            <code>{/^0+$/.test(row.oid) ? '未提交' : shortOid(row.oid)}</code>
            <span className="truncate">{row.author}</span>
            <span className="muted">{row.line}</span>
            <code>{row.content}</code>
          </div>
        )}
      />
    </div>
  );
}
