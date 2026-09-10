import { useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { FolderOpen, LoaderCircle } from 'lucide-react';
import { api, errorMessage } from '../api';
import { Modal } from './UI';

export default function CloneRepository({ onClose }: { onClose: () => void }) {
  const [url, setUrl] = useState('');
  const [path, setPath] = useState('');
  const [choosing, setChoosing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const locked = choosing || busy;

  const chooseDestination = async () => {
    setChoosing(true);
    setError('');
    try {
      const name = url
        .trim()
        .replace(/[?#].*$/, '')
        .replace(/\/+$/, '')
        .split(/[/:\\]/)
        .pop()
        ?.replace(/\.git$/i, '');
      const destination = await save({
        title: '选择克隆到本机的位置',
        defaultPath: path || (name && name !== '.' && name !== '..' ? name : 'my-repository'),
      });
      if (destination) setPath(destination);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setChoosing(false);
    }
  };

  return (
    <Modal title="从远程克隆仓库" onClose={locked ? undefined : onClose}>
      <form
        onSubmit={async (event) => {
          event.preventDefault();
          if (locked || !url.trim() || !path) return;
          setBusy(true);
          setError('');
          try {
            await api.create(path, url.trim());
            onClose();
          } catch (error) {
            setError(errorMessage(error));
          } finally {
            setBusy(false);
          }
        }}
      >
        <p className="dialog-description">填写远程仓库地址，再选择保存到本机的位置。</p>
        <label className="form-field">
          <span>远程仓库地址</span>
          <input
            autoFocus
            required
            value={url}
            disabled={locked}
            spellCheck={false}
            autoCapitalize="none"
            autoComplete="off"
            placeholder="https://github.com/team/repository.git"
            onChange={(event) => setUrl(event.target.value)}
          />
          <small className="muted">
            支持 HTTPS 或 SSH，例如 git@github.com:team/repository.git
          </small>
        </label>
        <div className="form-field">
          <label htmlFor="clone-destination">本地保存目录</label>
          <div className="clone-destination">
            <input
              id="clone-destination"
              readOnly
              value={path}
              placeholder="选择新仓库的保存位置…"
            />
            <button
              type="button"
              className="button"
              disabled={locked || !url.trim()}
              onClick={() => void chooseDestination()}
            >
              <FolderOpen size={14} />
              选择位置…
            </button>
          </div>
          <small className="muted">远程仓库的文件和提交历史会下载到这个目录。</small>
        </div>
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" disabled={locked} onClick={onClose}>
            取消
          </button>
          <button
            type="submit"
            className="button primary"
            disabled={locked || !url.trim() || !path}
          >
            {busy && <LoaderCircle size={14} className="spin" />}克隆到本机
          </button>
        </div>
      </form>
    </Modal>
  );
}
