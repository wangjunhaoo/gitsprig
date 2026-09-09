#!/usr/bin/env python3
"""打包 macOS 应用与 DMG，不通过 Finder 自动化修改镜像布局。"""
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[1]
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("当前发布脚本仅用于 Apple Silicon macOS。")
    version = json.loads((root / "package.json").read_text())["version"]
    subprocess.run(["python3", "scripts/collect_licenses.py", "--check"], cwd=root, check=True)
    subprocess.run(["npm", "run", "tauri", "--", "build", "--bundles", "app"], cwd=root, check=True)
    product_name = json.loads((root / "src-tauri/tauri.conf.json").read_text())["productName"]
    app = root / f"src-tauri/target/release/bundle/macos/{product_name}.app"
    executable = app / "Contents/MacOS/gitgui"
    if not executable.is_file():
        raise SystemExit("应用构建未生成可执行文件。")
    output = root / "artifacts/releases"
    output.mkdir(parents=True, exist_ok=True)
    arch = "aarch64" if platform.machine() == "arm64" else platform.machine()
    destination = output / f"{product_name}_{version}_{arch}.dmg"
    with tempfile.TemporaryDirectory(prefix="gitgui-package-") as temporary:
        staging = Path(temporary) / product_name
        staging.mkdir()
        shutil.copytree(app, staging / app.name, symlinks=True)
        shutil.copyfile(root / "LICENSE", staging / "LICENSE.txt")
        shutil.copyfile(root / "third-party/NOTICES.txt", staging / "THIRD-PARTY-NOTICES.txt")
        shutil.copyfile(root / "docs/INSTALL_MACOS.md", staging / "安装说明.md")
        (staging / "Applications").symlink_to("/Applications", target_is_directory=True)
        image = Path(temporary) / f"{product_name}.dmg"
        subprocess.run(["hdiutil", "create", "-volname", product_name, "-srcfolder", str(staging), "-format", "UDZO", "-ov", str(image)], check=True)
        subprocess.run(["hdiutil", "verify", str(image)], check=True)
        shutil.copyfile(image, destination)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    metadata = {
        "version": version, "architecture": arch, "app": app.name, "dmg": destination.name,
        "dmgBytes": destination.stat().st_size, "dmgMB": destination.stat().st_size / 1000000,
        "appBytes": sum(p.stat().st_size for p in app.rglob("*") if p.is_file()),
        "dmgSha256": hashlib.sha256(destination.read_bytes()).hexdigest(),
        "executableSha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
        "signature": "ad-hoc", "notarized": False,
    }
    (output / "manifest.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2))
    (output / "SHA256SUMS.txt").write_text(
        f'{metadata["dmgSha256"]}  {destination.name}\n', encoding="utf-8"
    )
    print(json.dumps(metadata, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
