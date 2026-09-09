#!/usr/bin/env python3
"""按锁文件收集 macOS ARM64 依赖的许可证与来源，缺失声明时停止。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
TARGET = "aarch64-apple-darwin"
PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE")


def license_files(directory):
    return sorted(
        path for path in directory.rglob("*")
        if path.is_file()
        and path.name.upper().startswith(PREFIXES)
        and path.suffix.lower() not in (".rs", ".toml", ".yml", ".lock")
    )


def build_notices():
    texts = {}
    components = []
    overrides = json.loads((ROOT / "third-party/upstream-sources.json").read_text())

    def collect(ecosystem, name, version, license_id, sources, files, authors):
        if not files or not license_id:
            raise RuntimeError(f"缺少许可证声明：{ecosystem}/{name}@{version}")
        documents = []
        for path, origin in files:
            raw = path.read_bytes()
            digest = hashlib.sha256(raw).hexdigest()
            texts[digest] = raw.decode("utf-8")
            documents.append({"source": origin, "sha256": digest})
        components.append({
            "ecosystem": ecosystem, "name": name, "version": version,
            "license": license_id, "authors": authors,
            "sources": sources, "documents": documents,
        })

    lock = json.loads((ROOT / "package-lock.json").read_text())
    for location, entry in lock["packages"].items():
        if not location or entry.get("dev"):
            continue
        directory = ROOT / location
        package = json.loads((directory / "package.json").read_text())
        if package["version"] != entry["version"]:
            raise RuntimeError(f"前端依赖与锁文件不一致：{location}，请先运行 npm ci")
        author = package.get("author", "")
        if isinstance(author, dict):
            author = author.get("name", "")
        sources = [f'https://www.npmjs.com/package/{package["name"]}/v/{package["version"]}']
        collect("npm", package["name"], package["version"], package.get("license"),
                sources, [(f, str(f.relative_to(directory))) for f in license_files(directory)],
                [author] if author else [])

    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--format-version", "1", "--locked",
        "--filter-platform", TARGET, "--manifest-path", str(ROOT / "src-tauri/Cargo.toml"),
    ], cwd=ROOT, text=True))
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    reachable = set()
    pending = [metadata["resolve"]["root"]]
    while pending:
        package_id = pending.pop()
        if package_id in reachable:
            continue
        reachable.add(package_id)
        pending.extend(nodes[package_id]["dependencies"])
    for package in metadata["packages"]:
        if package["id"] not in reachable or package["source"] is None:
            continue
        directory = Path(package["manifest_path"]).parent
        files = [(f, str(f.relative_to(directory))) for f in license_files(directory)]
        key = f'{package["name"]}@{package["version"]}'
        for entry in overrides.get(key, []):
            path = ROOT / "third-party" / entry["path"]
            if hashlib.sha256(path.read_bytes()).hexdigest() != entry["sha256"]:
                raise RuntimeError(f"上游许可证校验失败：{entry['path']}")
            files.append((path, entry["url"]))
        sources = [
            f'https://crates.io/crates/{package["name"]}/{package["version"]}',
            f'https://static.crates.io/crates/{package["name"]}/{package["name"]}-{package["version"]}.crate',
        ]
        if package["repository"]:
            sources.append(package["repository"])
        collect("cargo", package["name"], package["version"], package["license"],
                sources, files, package["authors"])

    components.sort(key=lambda c: (c["ecosystem"], c["name"], c["version"]))
    lines = [
        "GitSprig 第三方组件许可证与来源",
        f"目标：{TARGET}",
        "收集范围包括前端运行依赖、Rust 目标依赖和构建依赖，可能多于最终二进制实际链接的组件。",
        "原创代码采用 MIT 许可证；以下依赖使用各自声明的许可证。许可证原文保留原语言。",
        "各组件均使用锁文件对应的上游原版。MPL 组件的对应源码可通过列出的 .crate 地址取得。",
        "部分上游包只提供许可声明，所引用的标准条款在本文件中一并收录。",
        "相同原文按 SHA256 合并存储；每个组件的原始声明来源和文本编号保留在索引中。",
        "",
    ]
    for component in components:
        lines += [
            f'[{component["ecosystem"]}] {component["name"]} {component["version"]}',
            f'许可证：{component["license"]}',
            f'上游作者：{", ".join(component["authors"])}',
            *[f"源码/项目：{url}" for url in component["sources"]],
            *[f'声明：{d["source"]} → {d["sha256"]}' for d in component["documents"]],
            "",
        ]
    for digest, content in sorted(texts.items()):
        lines += ["=" * 72, f"文本 SHA256：{digest}", "=" * 72, content, ""]
    inventory = {"target": TARGET, "components": components}
    return {
        ROOT / "third-party/NOTICES.txt": "\n".join(lines),
        ROOT / "third-party/components.json": json.dumps(inventory, indent=2, ensure_ascii=False) + "\n",
    }, len(components)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="只检查已提交的生成结果是否与当前锁文件一致")
    args = parser.parse_args()
    outputs, count = build_notices()
    for path, content in outputs.items():
        if args.check:
            if not path.is_file() or path.read_bytes().decode("utf-8") != content:
                raise RuntimeError(f"许可证清单需要重新生成：{path.relative_to(ROOT)}")
        else:
            path.write_bytes(content.encode("utf-8"))
    print(f"已{'核对' if args.check else '收集'} {count} 个组件的许可证与来源。")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"许可证收集失败：{error}", file=sys.stderr)
        raise SystemExit(1)
