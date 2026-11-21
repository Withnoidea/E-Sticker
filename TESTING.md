# 测试与发布

## 环境

- 构建：Windows x64、Rust stable、MSVC Build Tools、Node.js/npm；先执行 `npm ci --ignore-scripts` 获取锁定的离线编辑器资源。
- 运行：Microsoft Edge WebView2 Evergreen Runtime；缺失时正文和设置会提示错误，便签墙仍可使用。运行不需要 Node.js 或 Python。
- 集成测试：Python 3、Pillow（`python -m pip install Pillow`）及已安装的 `agent-browser` CLI。测试通过本机 9225 端口操作隔离的 WebView2 实例，不启用生产默认调试端口。
- 原生测试会打开自己的窗口、暂时使用键盘并截图，请勿同时操作测试窗口。所有测试便签都通过 `ESTICKER_DATA_DIR` 隔离在项目的 `artifacts` 内，不读写真实便签。

## 自动检查

```powershell
npm ci --ignore-scripts
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

单元测试覆盖 Markdown 格式和编号、UTF-16 选区、原子保存和失败保留、旧数据兼容、搜索/排序/标签筛选与墙面不变、标签解析上限、归档回收站、所有设置持久化、目录/图片迁移和并发锁。

## 原生墙面与真实编辑器回归

```powershell
python -u scripts/editor_qa.py --exe target/release/e-sticker.exe --output artifacts/qa-release-v0.2.9
```

该脚本不会替换系统剪贴板。它复用原生窗口定位和鼠标工具，但编辑操作直接发给真实 WebView2 页面，不往隐藏的旧正文控件写测试内容。

脚本逐项验证：

1. 内嵌 Vditor 在真实原生窗口中启动，工具栏和离线资源正常加载。
2. 只打开再关闭的正文不被编辑器规范化，原 Markdown 字节内容保留。
3. 实际编辑区域的中文和 emoji 输入自动保存，不改变位置或标签。
4. 原生新建、标签、亲手落贴、正文搜索保持原有墙面内容与顺序。
5. 编辑模式、字号、自动保存延迟通过原生保存按钮落盘；取消不写入配置。
6. 源码模式与 Ctrl+S 保存表格、任务、代码和公式 Markdown。
7. 编辑器真实文件上传进入原生图片存储，像素不变，落盘使用相对引用。
8. 从真实工具栏切换到所见即所得，表格和本地图片正常渲染。
9. 页面安全策略拒绝远程图片加载。
10. 最小 780×700 窗口中的原生控件无重叠和截断，编辑器仍可使用。
11. 五秒自动保存计时尚未到时关窗，最后输入仍被同步并保存。
12. 模拟写盘失败，确认磁盘原文与编辑器草稿均保留；恢复后 Ctrl+S 可重试。

`scripts/native_qa.py --suite` 是 v0.2.7 原生正文的历史回归，依赖已被替换的正文控件及设置按钮，不能用于验收 v0.2.8。其剪贴板 PNG/DIB 旧结果不代表 WebView2 已通过同样格式的专项测试。手动验收仍需覆盖真实中文输入法组合态、系统剪贴板图片、拖放、多屏 DPI、WebView2 缺失、目录迁移、置顶/归档/回收站与故障恢复。模型单测继续覆盖原子保存、目录及图片迁移、锁和旧数据兼容。

输出目录保留 `results.json` 和截图；失败会保留现场及可见控件信息。截图仍需人工检查文字、颜色和视觉层次，不能仅以脚本退出码代替视觉验收。

## 打包与最终验证

工具栏提示与 PDF 专项回归：`python -u scripts/editor_export_qa.py --exe target/release/e-sticker.exe`。脚本在隔离数据目录中检查普通与最小窗口提示方向、保存对话框取消后重试，以及三种编辑模式实际生成 PDF；内容包含中文、任务列表、表格、公式、本地图片和长文分页。PDF 位于该次 `artifacts/export-qa-*` 目录，可打开检查排版或用 PDF 解析工具检查正文与页数。

```powershell
powershell -File scripts/package.ps1
Expand-Archive dist/E-Sticker-v0.2.9-windows-x64.zip artifacts/release-check-v0.2.9
python -u scripts/editor_qa.py --exe artifacts/release-check-v0.2.9/E-Sticker-v0.2.9-windows-x64/E-Sticker.exe --output artifacts/qa-packaged-v0.2.9
python -u scripts/editor_export_qa.py --exe artifacts/release-check-v0.2.9/E-Sticker-v0.2.9-windows-x64/E-Sticker.exe
```

必须测试压缩包实际解压出的 EXE，而不仅是 debug 产物。发布包附带 `SHA256SUMS.txt`；ZIP 同目录另有 `.sha256` 校验文件。升级不要混用旧版 EXE。
