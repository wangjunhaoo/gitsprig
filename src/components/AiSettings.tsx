import { useEffect, useState } from 'react';
import { api, errorMessage } from '../api';
import { Modal, Spinner } from './UI';
import type { AiSettings } from '../types';

export default function AiSettingsDialog({ onClose }: { onClose: () => void }) {
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [originalEndpoint, setOriginalEndpoint] = useState('');
  const [key, setKey] = useState('');
  const [removeKey, setRemoveKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    let alive = true;
    void api
      .aiSettings()
      .then((value) => {
        if (alive) {
          setSettings(value);
          setOriginalEndpoint(value.endpoint);
        }
      })
      .catch((error) => {
        if (alive) setError(errorMessage(error));
      });
    return () => {
      alive = false;
    };
  }, []);
  return (
    <Modal title="AI 提交说明设置" onClose={busy ? undefined : onClose}>
      {!settings && !error && <Spinner text="正在读取设置…" />}
      {settings && (
        <form
          onSubmit={async (event) => {
            event.preventDefault();
            setBusy(true);
            setError('');
            try {
              await api.saveAiSettings({
                endpoint: settings.endpoint,
                model: settings.model,
                instruction: settings.instruction,
                apiKey: key || null,
                removeKey,
              });
              setKey('');
              onClose();
            } catch (error) {
              setError(errorMessage(error));
            } finally {
              setBusy(false);
            }
          }}
        >
          <p className="dialog-description">
            点击「AI
            生成」时，将勾选的差异和必要上下文发送到下方服务。生成结果填入草稿，由你编辑和提交。
          </p>
          <label className="form-field">
            <span>接口地址</span>
            <input
              autoFocus
              required
              type="url"
              value={settings.endpoint}
              placeholder="https://api.openai.com/v1"
              onChange={(e) => {
                setSettings({ ...settings, endpoint: e.target.value });
                setRemoveKey(false);
              }}
            />
            <small className="muted">
              填写含版本前缀的 Base URL 或完整 /chat/completions 地址；支持本地 HTTP 服务。
            </small>
          </label>
          <label className="form-field">
            <span>模型名称</span>
            <input
              required
              value={settings.model}
              placeholder="填写服务提供的模型 ID"
              spellCheck={false}
              onChange={(e) => setSettings({ ...settings, model: e.target.value })}
            />
          </label>
          <label className="form-field">
            <span>
              API Key <span className="muted">（可选）</span>
            </span>
            <input
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={key}
              placeholder={
                settings.hasApiKey && settings.endpoint === originalEndpoint
                  ? '已保存在本地文件，留空保留现有密钥'
                  : '无需认证的本地服务可留空'
              }
              onChange={(e) => {
                setKey(e.target.value);
                setRemoveKey(false);
              }}
            />
            <small className="muted">
              配置和密钥保存在 ~/.gitgui/ai.json 中（明文，仅当前用户可读写）。
              更换接口地址时，原服务的密钥不会自动发送到新地址。
            </small>
          </label>
          {settings.hasApiKey && settings.endpoint === originalEndpoint && (
            <label className="form-field checkbox-field">
              <input
                type="checkbox"
                checked={removeKey}
                onChange={(e) => {
                  setRemoveKey(e.target.checked);
                  setKey('');
                }}
              />
              <span>移除这个接口已保存的密钥</span>
            </label>
          )}
          <label className="form-field">
            <span>
              生成偏好 <span className="muted">（可选）</span>
            </span>
            <textarea
              rows={3}
              value={settings.instruction}
              placeholder="例如：采用 Conventional Commits，中文标题不超过 50 字"
              onChange={(e) => setSettings({ ...settings, instruction: e.target.value })}
            />
          </label>
          {error && (
            <p className="inline-error" role="alert">
              {error}
            </p>
          )}
          <div className="dialog-actions">
            <button type="button" className="button" disabled={busy} onClick={onClose}>
              取消
            </button>
            <button type="submit" className="button primary" disabled={busy}>
              {busy ? '正在保存…' : '保存设置'}
            </button>
          </div>
        </form>
      )}
      {!settings && error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </Modal>
  );
}
