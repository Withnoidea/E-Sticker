# 第三方组件

## 正文编辑器

E-Sticker 使用 Vanessa219/vditor 项目的 Vditor 4.0.1 官方 npm 发布资源，锁定版本及完整性摘要记录在 `package-lock.json`。编辑器界面和原生桥接属于本项目代码；上游 Vditor 发布文件不作修改。

Vditor 使用 MIT 许可证，版权声明为 Copyright (c) 2019-present B3log 开源, b3log.org。完整许可证作为 `Vditor-LICENSE.txt` 附在发布包中，并内嵌到编辑器资源。

上游发布资源包含 Lute、highlight.js、KaTeX、MathJax、Mermaid 等渲染组件，保留其文件中的版权和许可证声明。Vditor 中使用的 diff-match-patch 为 Apache-2.0，其完整许可证以 `diff-match-patch-LICENSE.txt` 随发布包附带。

## Windows 容器

WebView2 使用操作系统中已安装的 Microsoft Edge WebView2 Evergreen Runtime；本包不携带或静默下载微软运行时。Rust 绑定来自 `webview2-com`，依赖版本记录于 `Cargo.lock`。
