import { useEffect, useRef, useState } from 'react';
import { api, errorMessage } from './api';
import { aiDraftDecision, selectionKey } from './logic';
import type { AiDraftContext } from './logic';
import type { Selection } from './types';

interface Props {
  repoId?: string;
  snapshot?: string;
  selections: Record<string, Selection>;
  message: string;
  setMessage: (value: string) => void;
  notify: (value: string) => void;
  fail: (error: unknown) => void;
}

export function useAiCommit(props: Props) {
  const [busy, setBusy] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [suggestion, setSuggestion] = useState<{ text: string; context: AiDraftContext } | null>(
    null,
  );
  const running = useRef<string | null>(null);
  const mounted = useRef(true);
  const current = useRef(props);
  current.current = props;
  const context = (): AiDraftContext => ({
    repoId: current.current.repoId,
    snapshot: current.current.snapshot,
    selection: selectionKey(current.current.selections),
    draft: current.current.message,
  });

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (running.current) void api.cancelAi(running.current).catch(() => {});
    };
  }, []);
  useEffect(() => {
    setSuggestion(null);
    if (running.current) void cancel();
  }, [props.repoId]);

  async function cancel() {
    const id = running.current;
    if (!id) return;
    // 先废弃本地结果，再通知服务层，避免已完成的响应覆盖草稿。
    running.current = null;
    try {
      await api.cancelAi(id);
    } catch (error) {
      current.current.fail(error);
    }
    if (mounted.current) setBusy(false);
  }

  async function generate() {
    if (busy || running.current) return;
    const before = context();
    if (!before.repoId || !before.snapshot) return;
    const selections = Object.values(current.current.selections);
    if (!selections.length) return;
    const id = crypto.randomUUID();
    running.current = id;
    setBusy(true);
    setSuggestion(null);
    try {
      const settings = await api.aiSettings();
      if (running.current !== id) return;
      if (!settings.endpoint || !settings.model) {
        setSettingsOpen(true);
        return;
      }
      if (aiDraftDecision(before, context()) === 'discard') return;
      const text = await api.generateMessage(before.repoId, id, before.snapshot, selections);
      if (!mounted.current || running.current !== id) return;
      const decision = aiDraftDecision(before, context());
      if (decision === 'apply') {
        current.current.setMessage(text);
        current.current.notify('已生成提交说明，可编辑后提交。');
      } else if (decision === 'preview') {
        setSuggestion({ text, context: before });
      } else {
        current.current.notify('生成期间仓库或选择发生变化，结果未填入，请重新生成。');
      }
    } catch (error) {
      if (mounted.current && running.current === id) current.current.fail(errorMessage(error));
    } finally {
      if (running.current === id) {
        running.current = null;
        if (mounted.current) setBusy(false);
      }
    }
  }

  async function applySuggestion() {
    if (!suggestion || !suggestion.context.repoId) return;
    if (aiDraftDecision(suggestion.context, context()) === 'discard') {
      current.current.fail('仓库或选择已变化，请关闭预览并重新生成。');
      return;
    }
    const status = await api.status(suggestion.context.repoId);
    if (
      status.snapshot !== suggestion.context.snapshot ||
      aiDraftDecision(suggestion.context, context()) === 'discard'
    ) {
      throw new Error('仓库或选择已变化，请关闭预览并重新生成。');
    }
    current.current.setMessage(suggestion.text);
    setSuggestion(null);
    current.current.notify('已填入生成的提交说明。');
  }

  return {
    busy,
    generate,
    cancel,
    settingsOpen,
    setSettingsOpen,
    suggestion,
    setSuggestion,
    applySuggestion,
  };
}
