# 参与贡献

欢迎提交问题、改进建议和 Pull Request。界面、说明和新增代码注释优先使用简体中文。

## 开发环境

当前开发目标为 macOS 14 及以上的 Apple Silicon，已完成的实机验收环境见 [验收记录](docs/ACCEPTANCE.md)。需要 Node.js 22、Git 2.40+、Python 3、Xcode 命令行工具，以及 rust-toolchain.toml 指定的 Rust 工具链。

```sh
env NODE_ENV=development npm ci --include=dev
npm run desktop
```

## 提交改动

1. Fork 仓库，从 main 创建用途明确的分支。
2. 保持改动聚焦；修复 Git 行为时先补能够复现问题的独立仓库测试。
3. 说明具体触发条件、修改后的行为和验证结果。界面改动附演示仓库截图。
4. 提交 Pull Request，并关联相关 Issue。

## 本地验证

```sh
npm run check:format
npm run build
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
npm run test:rust
npm run check:licenses
```

Rust 测试脚本先完成编译，再把测试分批运行；每批最多 60 秒。测试应创建临时仓库及本机模拟远程，不得访问或写入贡献者的真实业务仓库。

请重点保护已有暂存版本、未提交内容、特殊路径、冲突恢复和外部并发修改。前端只能调用明确的后端操作，不能拼接任意 shell 命令。

## 问题报告

请提供系统版本、CPU 架构、GitSprig 和 Git 版本、复现步骤，以及必要的错误信息。分享之前清除凭据、真实远程地址、业务代码和个人目录信息。不要把 AI 配置、Git 密钥或运行日志整体上传。

涉及凭据泄露、代码执行或数据丢失的安全问题，请按 [安全报告说明](SECURITY.md) 私下报告。

## 许可证

提交贡献即表示你有权提交相关内容，并同意你的贡献按本仓库的 MIT 许可证发布。第三方代码和资源应注明来源并保留其原许可证。
