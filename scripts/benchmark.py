#!/usr/bin/env python3
"""构建隔离的大仓库，并测量发布版冷启动及进程内存。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import time
from macos_footprint import process_memory


def git(root, *args, **kwargs):
    return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True, **kwargs)


def fixture(root):
    if root.exists():
        count = int(git(root, "rev-list", "--count", "HEAD", text=True).stdout.strip())
        files = len(git(root, "ls-files", "-z").stdout.split(b"\0")) - 1
        if count != 50000 or files != 10000:
            raise RuntimeError("已有目录不是此脚本创建的基准仓库，拒绝更改。")
        return
    root.mkdir(parents=True)
    git(root, "init", "-b", "main")
    git(root, "config", "user.name", "GitGUI Benchmark")
    git(root, "config", "user.email", "benchmark@example.invalid")
    git(root, "config", "commit.gpgsign", "false")
    lines = "".join(f"export const value{i} = {i};\n" for i in range(32)).encode()
    stream = bytearray(b"blob\nmark :1\ndata " + str(len(lines)).encode() + b"\n" + lines + b"\n")
    for index in range(50000):
        message = f"基准提交 {index + 1}\n".encode()
        timestamp = 1700000000 + index
        stream += f"commit refs/heads/main\nmark :{index+2}\ncommitter GitGUI Benchmark <benchmark@example.invalid> {timestamp} +0800\ndata {len(message)}\n".encode() + message
        if index:
            stream += f"from :{index+1}\n".encode()
        else:
            for file_index in range(10000):
                stream += f"M 100644 :1 src/module-{file_index//100:03d}/file-{file_index:05d}.ts\n".encode()
        stream += b"\n"
    git(root, "fast-import", "--quiet", input=bytes(stream))
    git(root, "reset", "--hard", "HEAD")
    for index in range(200):
        path = root / f"src/module-{index//100:03d}/file-{index:05d}.ts"
        path.write_bytes(lines.replace(b"value4 = 4", b"value4 = 42"))
    print(json.dumps({"fixture": str(root), "files": 10000, "commits": 50000, "modified": 200}), flush=True)


def processes():
    output = subprocess.run(["ps", "-axo", "pid=,ppid=,rss=,%cpu=,comm="], capture_output=True, text=True, check=True).stdout
    result = {}
    for row in output.splitlines():
        fields = row.strip().split(None, 4)
        if len(fields) != 5:
            continue
        result[int(fields[0])] = {"ppid": int(fields[1]), "rssKb": int(fields[2]), "cpu": float(fields[3]), "command": fields[4]}
    return result


def owned(snapshot, before, pid):
    ids = {pid}
    changed = True
    while changed:
        changed = False
        for child, row in snapshot.items():
            if row["ppid"] in ids and child not in ids:
                ids.add(child)
                changed = True
    # WebKit 的 XPC 进程父进程通常为 launchd，采用启动前后进程差集计入。
    for child, row in snapshot.items():
        if child not in before and "WebKit" in row["command"]:
            ids.add(child)
    return {child: snapshot[child] for child in ids if child in snapshot}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", type=Path, required=True)
    parser.add_argument("--executable", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--idle-seconds", type=float, default=30)
    parser.add_argument("--launch-mode", choices=["foreground", "direct"], default="foreground")
    args = parser.parse_args()
    fixture(args.repository)
    args.output.mkdir(parents=True, exist_ok=True)
    if not args.executable:
        return
    executable = args.executable.resolve()
    executable_hash = hashlib.sha256(executable.read_bytes()).hexdigest()
    records = []
    known_webkit = set()
    for index in range(args.runs):
        data_dir = args.output / f"run-{index+1}"
        data_dir.mkdir(exist_ok=True)
        prefs = {"recent": [], "groups": {}, "theme": "dark", "layout": {"sidebar": 290, "history": 340}, "drafts": {}, "lastPath": str(args.repository.resolve())}
        (data_dir / "preferences.json").write_text(json.dumps(prefs))
        metrics_file = data_dir / "launch-metrics.json"
        if metrics_file.exists():
            metrics_file.unlink()
        before = processes()
        before = {pid: row for pid, row in before.items() if pid not in known_webkit}
        log = (data_dir / "process.log").open("w")
        environment = {**os.environ, "GITGUI_DATA_DIR": str(data_dir.resolve())}
        started = time.perf_counter()
        if args.launch_mode == "foreground":
            app_bundle = executable.parents[2]
            if app_bundle.suffix != ".app":
                raise RuntimeError("前台启动需要 .app 内部的可执行文件。")
            process = subprocess.Popen(["/usr/bin/open", "-n", "-W", "-a", str(app_bundle), "--env", f"GITGUI_DATA_DIR={data_dir.resolve()}"], env=environment, stdout=log, stderr=log)
            app_pid = None
        else:
            process = subprocess.Popen([str(executable)], env=environment, stdout=log, stderr=log)
            app_pid = process.pid
        try:
            metric = None
            while time.perf_counter() - started < 30:
                if process.poll() is not None:
                    raise RuntimeError(f"应用提前退出：{process.returncode}")
                if metrics_file.exists():
                    try:
                        data = json.loads(metrics_file.read_text())
                        if data["pid"] not in before and data["repository"]:
                            app_pid = data["pid"]
                            metric = data
                            break
                    except (json.JSONDecodeError, KeyError):
                        pass
                time.sleep(.01)
            if metric is None:
                raise RuntimeError("30秒内没有收到仓库可操作标记。")
            startup = (time.perf_counter() - started) * 1000
            time.sleep(args.idle_seconds)
            samples = []
            for _ in range(5):
                snapshot = owned(processes(), before, app_pid)
                for pid, row in snapshot.items():
                    try:
                        row.update(process_memory(pid))
                    except OSError as error:
                        row["footprintError"] = str(error)
                footprint = sum(row["footprintBytes"] for row in snapshot.values()) / 1000000 if all("footprintBytes" in row for row in snapshot.values()) else None
                samples.append({"memoryMB": sum(row["rssKb"] * 1024 for row in snapshot.values()) / 1000000, "footprintMB": footprint, "cpuPercent": sum(row["cpu"] for row in snapshot.values()), "processes": snapshot})
                known_webkit.update(pid for pid, row in snapshot.items() if "WebKit" in row["command"])
                time.sleep(.5)
            record = {"run": index + 1, "startupMs": startup, "appReadyMs": metric["startupMs"], "idleMemoryMB": statistics.median(sample["memoryMB"] for sample in samples), "idleCpuPercent": statistics.median(sample["cpuPercent"] for sample in samples), "processes": samples[-1]["processes"]}
            record["idleFootprintMB"] = statistics.median(sample["footprintMB"] for sample in samples) if all(sample["footprintMB"] is not None for sample in samples) else None
            records.append(record)
            record["frontendTiming"] = metric.get("frontend")
            trim_file = data_dir / "memory-trim.json"
            if trim_file.exists():
                record["memoryTrim"] = json.loads(trim_file.read_text())
            (data_dir / "result.json").write_text(json.dumps(record, ensure_ascii=False, indent=2))
            print(json.dumps(record, ensure_ascii=False), flush=True)
        finally:
            if app_pid is None and args.launch_mode == "foreground":
                candidates = [pid for pid, row in processes().items() if pid not in before and row["command"] == str(executable)]
                if len(candidates) == 1:
                    app_pid = candidates[0]
            if app_pid is not None:
                try:
                    os.kill(app_pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                if app_pid is not None:
                    try:
                        os.kill(app_pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                process.kill()
                process.wait()
            log.close()
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline and known_webkit.intersection(processes()):
            time.sleep(.2)
        time.sleep(1)
    if hashlib.sha256(executable.read_bytes()).hexdigest() != executable_hash:
        raise RuntimeError("测量期间应用被重新构建，当前结果作废。")
    startup_times = sorted(record["startupMs"] for record in records)
    report = {
        "platform": platform.platform(), "architecture": platform.machine(),
        "executableSha256": executable_hash,
        "launchMode": args.launch_mode,
        "repository": str(args.repository), "trackedFiles": 10000, "commits": 50000, "modifiedFiles": 200,
        "measurement": "进程冷启动（未清空系统磁盘缓存）；从启动进程至仓库列表可操作标记；内存为主进程、后代及启动后新增WebKit进程的RSS合计。",
        "startupMedianMs": statistics.median(startup_times), "startupMaxMs": max(startup_times),
        "idleMemoryMedianMB": statistics.median(row["idleMemoryMB"] for row in records),
        "idleMemoryMaxMB": max(row["idleMemoryMB"] for row in records), "runs": records,
        "idleFootprintMedianMB": statistics.median(row["idleFootprintMB"] for row in records) if all(row["idleFootprintMB"] is not None for row in records) else None,
    }
    (args.output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps({k: v for k, v in report.items() if k != "runs"}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
