import { useEffect, useMemo, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ArrowDown, GitBranch, LoaderCircle, RefreshCw } from 'lucide-react';
import { api, errorMessage } from '../api';
import type {
  OperationEvent,
  PushRemoteInfo,
  Reference,
  Remote,
  Repository,
  RepositoryStatus,
} from '../types';
import { Modal } from './UI';
import SearchSelect from './SearchSelect';

export default function PushDialog({
  repo,
  status,
  references,
  remotes,
  force: initialForce,
  busy,
  onClose,
}: {
  repo: Repository;
  status: RepositoryStatus;
  references: Reference[];
  remotes: Remote[];
  force: boolean;
  busy: boolean;
  onClose: () => void;
}) {
  const sources = useMemo(() => {
    const local = references.filter((reference) => reference.kind === 'local');
    if (status.branch === '(detached)' && status.head)
      local.unshift({
        name: '游离 HEAD',
        fullName: 'HEAD',
        oid: status.head,
        current: true,
        upstream: '',
        kind: 'local',
      });
    return local;
  }, [references, status.branch, status.head]);
  const initial = sources.find((reference) => reference.current) ?? sources[0];
  const remoteFor = (source?: Reference) =>
    [...remotes]
      .sort((a, b) => b.name.length - a.name.length)
      .find((remote) => source?.upstream.startsWith(remote.name + '/'));
  const initialRemote = remoteFor(initial) ?? remotes[0];
  const targetFor = (source: Reference | undefined, remote: string) =>
    source?.upstream.startsWith(remote + '/')
      ? source.upstream.slice(remote.length + 1)
      : source?.fullName === 'HEAD'
        ? ''
        : (source?.name ?? '');
  const [source, setSource] = useState(initial?.fullName ?? '');
  const [remote, setRemote] = useState(initialRemote?.name ?? '');
  const [target, setTarget] = useState(targetFor(initial, initialRemote?.name ?? ''));
  const [setUpstream, setSetUpstream] = useState(
    !initial?.upstream && initial?.fullName !== 'HEAD',
  );
  const [force, setForce] = useState(initialForce);
  const [info, setInfo] = useState<PushRemoteInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [queryError, setQueryError] = useState('');
  const [error, setError] = useState('');
  const [refresh, setRefresh] = useState(0);
  const [submitting, setSubmitting] = useState(false);
  const currentSource = sources.find((reference) => reference.fullName === source);

  useEffect(() => {
    if (!remote) return;
    let alive = true;
    let operationId: string | undefined;
    let unlisten: (() => void)[] = [];
    const requestId = crypto.randomUUID();
    setLoading(true);
    setInfo(null);
    setQueryError('');
    void Promise.all([
      listen<{ requestId: string; info?: PushRemoteInfo; error?: string }>(
        'push-remote-result',
        ({ payload }) => {
          if (!alive || payload.requestId !== requestId) return;
          setLoading(false);
          if (payload.info) setInfo(payload.info);
          else setQueryError(payload.error ?? '无法读取远程分支');
        },
      ),
      listen<OperationEvent>('git-operation', ({ payload }) => {
        if (!alive || payload.repoId !== 'push-query:' + requestId) return;
        if (payload.state === 'error' || payload.state === 'cancelled') {
          setLoading(false);
          setQueryError(payload.message);
        }
      }),
    ])
      .then(async (handlers) => {
        unlisten = handlers;
        if (!alive) {
          handlers.forEach((handler) => handler());
          return;
        }
        operationId = await api.queryPushRemote(repo.id, remote, requestId);
        if (!alive) void api.cancel(operationId).catch(() => {});
      })
      .catch((error) => {
        if (alive) {
          setLoading(false);
          setQueryError(errorMessage(error));
        }
      });
    return () => {
      alive = false;
      unlisten.forEach((handler) => handler());
      if (operationId) void api.cancel(operationId).catch(() => {});
    };
  }, [repo.id, remote, refresh]);

  const branch = info?.branches.find((branch) => branch.name === target);
  const canPush =
    !submitting &&
    !busy &&
    !loading &&
    !!info &&
    info.remote === remote &&
    !!currentSource &&
    !!target.trim() &&
    (!force || !!branch?.oid);
  return (
    <Modal title="推送到远程仓库" onClose={submitting ? undefined : onClose}>
      <form
        className="push-form"
        onSubmit={async (event) => {
          event.preventDefault();
          if (!canPush || !info || !currentSource) return;
          setSubmitting(true);
          setError('');
          try {
            await api.push(repo.id, {
              remote,
              configuration: info.configuration,
              source,
              sourceOid: currentSource.oid,
              target: target.trim(),
              force,
              expected: branch?.oid ?? null,
              setUpstream: setUpstream && source !== 'HEAD',
            });
            onClose();
          } catch (error) {
            setError(errorMessage(error));
          } finally {
            setSubmitting(false);
          }
        }}
      >
        <p className="dialog-description">
          选择本地分支和远程目标。目标列表直接读取推送地址，包含尚未获取到本地的分支。
        </p>
        <SearchSelect
          label="本地分支"
          value={source}
          disabled={submitting}
          options={sources.map((reference) => ({
            value: reference.fullName,
            label: reference.name,
            description: (reference.current ? '当前分支 · ' : '') + reference.oid.slice(0, 8),
          }))}
          onChange={(value) => {
            const next = sources.find((reference) => reference.fullName === value);
            const nextRemote = remoteFor(next)?.name ?? remote;
            setSource(value);
            setRemote(nextRemote);
            setTarget(targetFor(next, nextRemote));
            setSetUpstream(!next?.upstream && value !== 'HEAD');
          }}
        />
        <div className="push-direction">
          <ArrowDown size={16} />
          <span>推送到</span>
        </div>
        <SearchSelect
          label="远程仓库"
          value={remote}
          disabled={submitting}
          options={remotes.map((remote) => ({
            value: remote.name,
            label: remote.name,
            description: remote.pushUrl.replace(/(\/\/)[^/@]+@/g, '$1***@'),
          }))}
          onChange={(value) => {
            setRemote(value);
            setTarget(targetFor(currentSource, value));
          }}
        />
        <SearchSelect
          label="目标分支"
          value={target}
          disabled={submitting || loading || !info}
          placeholder={loading ? '正在读取远程分支…' : '搜索已有分支或输入新分支名…'}
          allowCreate
          options={(info?.branches ?? []).map((branch) => ({
            value: branch.name,
            label: branch.name,
            description: branch.oid
              ? branch.oid.slice(0, 8)
              : `位于 ${branch.destinations}/${info!.urls.length} 个推送地址，提交不完全相同`,
          }))}
          onChange={setTarget}
        />
        <div className="push-remote-status">
          {loading ? (
            <>
              <LoaderCircle size={12} className="spin" />
              <span>正在读取 {remote} 的分支…</span>
            </>
          ) : (
            <span>
              {info
                ? `${info.branches.length} 个远程分支${target && !branch ? ' · 将新建目标分支' : ''}`
                : '远程分支读取失败'}
            </span>
          )}
          <span className="flex-spacer" />
          <button
            type="button"
            className="text-button"
            disabled={submitting}
            onClick={() => setRefresh((value) => value + 1)}
          >
            <RefreshCw size={12} />
            刷新分支
          </button>
        </div>
        {queryError && (
          <div className="inline-error" role="alert">
            {queryError}
          </div>
        )}
        {info && info.urls.length > 1 && (
          <p className="push-note">
            此远程配置了 {info.urls.length} 个推送地址，本次将按 Git 配置推送到全部地址。
          </p>
        )}
        <div className="push-summary">
          <GitBranch size={14} />
          <span>
            {currentSource?.name ?? '未选择来源'} <b>→</b> {remote}/{target || '未选择目标'}
          </span>
        </div>
        <label className="push-option">
          <input
            type="checkbox"
            checked={setUpstream}
            disabled={submitting || source === 'HEAD'}
            onChange={(event) => setSetUpstream(event.target.checked)}
          />
          设为该本地分支的上游
        </label>
        <label className="push-option">
          <input
            type="checkbox"
            checked={force}
            disabled={submitting}
            onChange={(event) => setForce(event.target.checked)}
          />
          安全强制推送
        </label>
        {force && (
          <p className="push-force-note">
            {branch?.oid
              ? `将用所选本地提交替换 ${remote}/${target}。仅当远端仍是 ${branch.oid.slice(0, 8)} 时允许覆盖。`
              : '请先选择已存在且各推送地址提交一致的远程分支。新分支使用普通推送。'}
          </p>
        )}
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <div className="dialog-actions">
          <button type="button" className="button" disabled={submitting} onClick={onClose}>
            取消
          </button>
          <button
            type="submit"
            disabled={!canPush}
            className={`button ${force ? 'danger' : 'primary'}`}
          >
            {submitting && <LoaderCircle size={14} className="spin" />}
            {force ? '确认强制推送' : '推送分支'}
          </button>
        </div>
      </form>
    </Modal>
  );
}
