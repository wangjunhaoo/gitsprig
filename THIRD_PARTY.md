# 第三方组件说明

本项目的 MIT 许可证覆盖原创代码。依赖保留各自的许可证；以下为直接运行依赖的索引，不替代各项目的完整许可证。传递依赖和精确版本以锁文件为准。

## 前端直接依赖

| 组件                        | 锁定版本 | 许可证            |
| --------------------------- | -------- | ----------------- |
| @codemirror/commands        | 6.11.0   | MIT               |
| @codemirror/lang-css        | 6.3.1    | MIT               |
| @codemirror/lang-html       | 6.4.12   | MIT               |
| @codemirror/lang-javascript | 6.2.5    | MIT               |
| @codemirror/lang-json       | 6.0.2    | MIT               |
| @codemirror/lang-markdown   | 6.5.2    | MIT               |
| @codemirror/lang-python     | 6.2.1    | MIT               |
| @codemirror/lang-rust       | 6.0.2    | MIT               |
| @codemirror/language        | 6.12.4   | MIT               |
| @codemirror/merge           | 6.12.2   | MIT               |
| @codemirror/search          | 6.7.2    | MIT               |
| @codemirror/state           | 6.7.4    | MIT               |
| @codemirror/view            | 6.43.11  | MIT               |
| @lezer/highlight            | 1.2.3    | MIT               |
| @tanstack/react-virtual     | 3.14.11  | MIT               |
| @tauri-apps/api             | 2.11.1   | Apache-2.0 OR MIT |
| @tauri-apps/plugin-dialog   | 2.7.3    | MIT OR Apache-2.0 |
| lucide-react                | 0.468.0  | ISC               |
| react                       | 19.2.8   | MIT               |
| react-dom                   | 19.2.8   | MIT               |

## Rust 直接依赖

| 组件                | 锁定版本 | 许可证            |
| ------------------- | -------- | ----------------- |
| base64              | 0.22.1   | MIT OR Apache-2.0 |
| libc                | 0.2.189  | MIT OR Apache-2.0 |
| notify              | 8.2.0    | CC0-1.0           |
| reqwest             | 0.12.28  | MIT OR Apache-2.0 |
| serde               | 1.0.229  | MIT OR Apache-2.0 |
| serde_json          | 1.0.151  | MIT OR Apache-2.0 |
| sha2                | 0.10.9   | MIT OR Apache-2.0 |
| similar             | 2.7.0    | Apache-2.0        |
| tauri               | 2.11.5   | Apache-2.0 OR MIT |
| tauri-build         | 2.6.3    | Apache-2.0 OR MIT |
| tauri-plugin-dialog | 2.7.3    | Apache-2.0 OR MIT |
| tempfile            | 3.27.0   | MIT OR Apache-2.0 |
| tokio               | 1.53.1   | MIT               |
| uuid                | 1.26.0   | Apache-2.0 OR MIT |

## 系统组件

GitSprig 使用系统 Git 和系统 WebView，不在源码仓库中复制或捆绑它们。构建工具及其他传递依赖也适用各自许可证。重新分发包含第三方组件的二进制时，应保留对应版权和许可证声明。

## 分发声明

[完整第三方声明](third-party/NOTICES.txt) 包含 macOS ARM64 目标的前端运行依赖、Rust 依赖及构建依赖，按原文内容去重；每个组件的版本、来源和声明编号见文件前半部分，也可读取 [机器可读清单](third-party/components.json)。清单范围可能大于最终二进制实际链接的组件。

上游依赖保持锁文件指定的原版。MPL 组件对应的源码下载地址随声明列出。个别 crate 没有打包许可证原文，补充文件取自该 crate 发布时的上游提交，来源与校验值保存在 [上游来源索引](third-party/upstream-sources.json)。

使用 `npm run licenses` 从已安装的锁定依赖生成清单，`npm run check:licenses` 检查清单是否过期。升级依赖后如缺少许可证，脚本会停止，需先补齐来源再生成。上游许可证原文不进行格式化。

打包脚本会验证清单，并将原创 MIT 许可证、完整第三方声明一起放入 DMG 和应用的 `Contents/Resources/licenses/` 目录。
