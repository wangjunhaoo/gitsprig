#!/usr/bin/env python3
"""先编译 Rust 测试，再分批执行；每批测试最多运行 60 秒。"""
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys


def run_batch(command, root):
    process = subprocess.Popen(
        command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, start_new_session=True,
    )
    try:
        output, _ = process.communicate(timeout=60)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        output, _ = process.communicate()
        print(output, flush=True)
        raise SystemExit("测试批次超过 60 秒，已终止。")
    print(output, flush=True)
    if process.returncode:
        raise SystemExit(process.returncode)
    return output


def main():
    root = Path(__file__).resolve().parents[1]
    command = [
        "cargo", "test", "--manifest-path", "src-tauri/Cargo.toml",
        "--locked", "--no-run", "--message-format=json",
    ]
    compiler = subprocess.Popen(command, cwd=root, stdout=subprocess.PIPE, text=True)
    executables = []
    for line in compiler.stdout:
        event = json.loads(line)
        if event.get("reason") == "compiler-message":
            rendered = event.get("message", {}).get("rendered")
            if rendered:
                print(rendered, file=sys.stderr)
        if event.get("reason") == "compiler-artifact" and event.get("profile", {}).get("test"):
            executable = event.get("executable")
            if executable:
                executables.append((event["target"]["name"], executable))
    if compiler.wait():
        raise SystemExit("Rust 测试编译失败。")
    total = 0
    for name, executable in executables:
        listing = run_batch([executable, "--list", "--format=terse"], root)
        tests = [line[:-6] for line in listing.splitlines() if line.endswith(": test")]
        for offset in range(0, len(tests), 12):
            batch = tests[offset:offset + 12]
            print(f"运行 {name}：第 {offset + 1}–{offset + len(batch)} 项", flush=True)
            output = run_batch([executable, "--exact", *batch, "--test-threads=2"], root)
            result = re.search(r"test result: ok\. (\d+) passed; 0 failed; 0 ignored", output)
            if not result or int(result.group(1)) != len(batch):
                raise SystemExit("实际执行的测试数量与批次不符，拒绝跳过测试。")
            total += len(batch)
    if not total:
        raise SystemExit("没有发现 Rust 测试，拒绝将空结果视为通过。")
    print(f"全部 {total} 项 Rust 测试通过。", flush=True)


if __name__ == "__main__":
    main()
