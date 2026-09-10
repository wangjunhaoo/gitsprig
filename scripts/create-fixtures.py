#!/usr/bin/env python3
"""创建独立的桌面验收仓库；拒绝覆盖已有目录。"""
import argparse
import json
from pathlib import Path
import subprocess


def git(path, *args, check=True):
    result = subprocess.run(["git", "-C", str(path), *args], capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(result.stderr)
    return result.stdout.strip()


def write(root, path, text):
    file = root / path
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(text)


def commit(root, message):
    git(root, "add", "-A")
    git(root, "commit", "-m", message)


def initialize(root):
    root.mkdir(parents=True)
    git(root, "init", "-b", "main")
    git(root, "config", "user.name", "GitGUI 验收")
    git(root, "config", "user.email", "qa@example.invalid")
    git(root, "config", "commit.gpgsign", "false")
    git(root, "config", "core.hooksPath", ".git/hooks")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    root = args.directory.resolve()
    if root.exists():
        raise SystemExit("目标目录已存在，拒绝覆盖。")
    root.mkdir(parents=True)
    workspace = root / "atlas-workspace"
    initialize(workspace)
    write(workspace, "README.md", "# Atlas\n\n轻量任务管理服务。\n")
    write(workspace, "package.json", '{\n  "name": "atlas",\n  "version": "1.0.0"\n}\n')
    write(workspace, "src/auth/session.ts", '''export interface Session {
  id: string;
  userId: string;
  expiresAt: number;
}

const SESSION_DURATION = 30 * 60 * 1000;

export function createSession(userId: string): Session {
  return {
    id: crypto.randomUUID(),
    userId,
    expiresAt: Date.now() + SESSION_DURATION,
  };
}

export function isExpired(session: Session): boolean {
  return session.expiresAt < Date.now();
}

export function refreshSession(session: Session): Session {
  return { ...session, expiresAt: Date.now() + SESSION_DURATION };
}
''')
    write(workspace, "src/config.ts", 'export const config = {\n  port: 3000,\n  timeout: 5000,\n  retries: 2,\n};\n')
    write(workspace, "src/utils/format.ts", 'export function formatDate(date: Date) {\n  return date.toISOString();\n}\n')
    write(workspace, "src/styles/theme.css", ':root {\n  --accent: #4778d7;\n  --background: #1e1f22;\n}\n')
    commit(workspace, "初始化项目与基础模块")
    git(workspace, "tag", "v1.0.0")
    git(workspace, "switch", "-c", "feature/session-refresh")
    for index, message in enumerate(["抽离会话配置", "补充请求超时处理", "增加会话过期检查"]):
        write(workspace, "docs/notes.md", f"# 开发记录\n\n步骤 {index + 1}：{message}\n")
        commit(workspace, message)
    git(workspace, "switch", "main")
    write(workspace, "docs/release.md", "# 发布记录\n\n准备 1.1 版本。\n")
    commit(workspace, "记录版本发布流程")
    git(workspace, "merge", "--no-ff", "--no-edit", "feature/session-refresh")
    git(workspace, "switch", "feature/session-refresh")
    source = (workspace / "src/auth/session.ts").read_text()
    source = source.replace("30 * 60", "60 * 60")
    source = source.replace("  return session.expiresAt < Date.now();", "  const now = Date.now();\n  return session.expiresAt <= now;")
    source += '\nexport function remainingTime(session: Session): number {\n  return Math.max(0, session.expiresAt - Date.now());\n}\n'
    write(workspace, "src/auth/session.ts", source)
    write(workspace, "src/config.ts", 'export const config = {\n  port: 3000,\n  timeout: 10000,\n  retries: 3,\n};\n')
    write(workspace, "src/styles/theme.css", ':root {\n  --accent: #5488e8;\n  --background: #1e1f22;\n  --surface: #25272b;\n}\n')
    write(workspace, "src/auth/session.test.ts", "import { isExpired } from './session';\n\nconst expired = { id: 'test', userId: 'u1', expiresAt: 0 };\nconsole.assert(isExpired(expired));\n")
    write(workspace, "README.md", "# Atlas\n\n轻量任务管理服务。\n\n支持会话自动续期。\n")
    git(workspace, "add", "README.md")
    remote = root / "origin.git"
    git(root, "init", "--bare", str(remote))
    git(workspace, "remote", "add", "origin", str(remote))
    git(workspace, "push", "-u", "origin", "main")
    git(workspace, "push", "-u", "origin", "feature/session-refresh")
    for kind in ["merge", "rebase"]:
        repo = root / f"{kind}-conflict"
        initialize(repo)
        write(repo, "settings.json", '{\n  "timeout": 5000,\n  "retries": 2,\n  "theme": "dark"\n}\n')
        commit(repo, "初始配置")
        git(repo, "switch", "-c", "feature/retry-policy")
        write(repo, "settings.json", '{\n  "timeout": 8000,\n  "retries": 4,\n  "theme": "dark"\n}\n')
        commit(repo, "调整重试策略")
        git(repo, "switch", "main")
        write(repo, "settings.json", '{\n  "timeout": 10000,\n  "retries": 3,\n  "theme": "dark"\n}\n')
        commit(repo, "更新默认超时")
        if kind == "merge":
            git(repo, "merge", "--no-edit", "feature/retry-policy", check=False)
        else:
            git(repo, "switch", "feature/retry-policy")
            git(repo, "rebase", "main", check=False)
    print(json.dumps({"workspace": str(workspace), "merge": str(root / "merge-conflict"), "rebase": str(root / "rebase-conflict"), "remote": str(remote)}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
