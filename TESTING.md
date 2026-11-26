# 测试与发布

## 环境

- 构建：Windows x64、Rust stable、MSVC Build Tools、Node.js/npm；先执行 `npm ci --ignore-scripts` 获取锁定的离线编辑器资源。
- 运行：Microsoft Edge WebView2 Evergreen Runtime；缺失时正文和设置会提示错误，便签墙仍可使用。运行不需要 Node.js 或 Python。
- 集成测试：Python 3、Pillow（`python -m pip install Pillow`）及已安装的 `agent-browser` CLI。测试通过本机 9225 端口操作隔离的 WebView2 实例，不启用生产默认调试端口。
- 原生测试会打开自己的窗口、暂时使用键盘并截图，请勿同时操作测试窗口。所有测试便签都通过 `ESTICKER_DATA_DIR` 隔离在项目的 `artifacts` 内，不读写真实便签。

## 自动检查

```powershell
npm ci --ignore-scripts
node --test scripts/markdown_test.cjs
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

单元测试覆盖 Markdown 格式和编号、UTF-16 选区、原子保存和失败保留、旧数据兼容、搜索/排序/标签筛选与墙面不变、标签解析上限、归档回收站、所有设置持久化、目录/图片迁移和并发锁。

## 多墙原生窗口回归

```powershell
python -u scripts/multiwall_qa.py --exe target/release/e-sticker.exe --output artifacts/qa-multiwall-v0.3.1
```

脚本验证 12 组真实窗口流程：隐藏默认分类、新建/重命名/取消、旧数据字节备份、分类墙落贴、两边拖动与层级独立、全局搜索打开被遮挡文章、列表排序与分类筛选、同一文章编辑与移动、删墙确认和归档内容进回收站、恢复到默认首页并重新分类、右键取消分类、首页直接创建、重名/写盘失败重试、780×700 布局及重启持久化（相关检查按流程分组）。

原生菜单使用实际鼠标点击，临时按线程切换 DPI 查询坐标；结束后恢复鼠标位置。请勿同时操作测试窗口。每次运行创建独立目录，`results.json` 记录被测 EXE 路径和 SHA-256，截图覆盖首页、分类墙、列表、回收站与最小窗口。

模型新增回归覆盖一文一墙、分类独立布局与层级溢出、增量首页不重排、分类筛选、删墙全量回收、恢复默认归属、v1/v2 备份迁移和备份失败保护、非法 v3 引用、倾斜命中测试。

## 原生墙面与真实编辑器回归

```powershell
python -u scripts/editor_qa.py --exe target/release/e-sticker.exe --output artifacts/qa-release-v0.3.1
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
13. 正文刚输入、五秒自动保存计时未到时执行分类移动，验证原生操作等待 WebView2 同步；在分类墙重新打开仍是同一篇正文，移回默认归属不复制文章或改变首页位置。

`scripts/native_qa.py --suite` 是 v0.2.7 原生正文的历史回归，依赖已被替换的正文控件和旧视图循环，不能用于验收 v0.3.0。新脚本只复用其底层窗口工具。其剪贴板 PNG/DIB 旧结果不代表 WebView2 已通过同样格式的专项测试。额外手动验收范围包括真实中文输入法组合态、系统剪贴板图片、系统拖放、多屏 DPI、WebView2 缺失及不同 Windows 环境；模型单测继续覆盖原子保存、目录及图片迁移、锁和旧数据兼容。

输出目录保留 `results.json` 和截图；失败会保留现场及可见控件信息。截图仍需人工检查文字、颜色和视觉层次，不能仅以脚本退出码代替视觉验收。

## 列表、排序与控件交互

```powershell
python -u scripts/interaction_qa.py --exe target/release/e-sticker.exe --output artifacts/qa-interaction-v0.3.1
```

该专项包含 11 组检查，使用真实回车事件，覆盖工具栏和快捷键转换无序/有序/任务列表后直接续写、空项退出、缩进/引用保留、已完成任务重置、代码围栏不误续写、Shift+Enter、组合态事件不接管、撤销重做与 Ctrl+S 落盘，并验证另外两种编辑模式未被破坏。自动化的组合态事件检查不等于真实中文输入法验收。

排序夹具让创建与编辑时间不同，核对置顶优先、时间倒序、同秒 ID 稳定排序及墙面数据不变。截图检查日期字段与所选排序一致。控件覆盖原生下拉框方向键/回车/Esc、设置选项、菜单 Esc 不误返回、浅色与深色菜单、最小窗口和删除弹窗真正的 Enter 默认取消/Esc 取消/显式确认。`results.json` 记录被测 EXE 的路径、SHA-256 与通过项。

源码模式续写接入锁定的 Vditor 4.0.1 `vditor.undo`，在插入前后记录快照；更新编辑器依赖时必须重跑撤销重做和直接回车用例。设置选项使用渐进增强的 `base-select`，旧 WebView2 保留原生展开列表与相同收起态样式。

## 打包与最终验证

默认打包目录为 `dist/`。如果其中的 EXE 正在运行，可用 `powershell -File scripts/package.ps1 -OutputDirectory artifacts/release-build` 在独立目录生成同名 ZIP 与校验文件，无需强制关闭现有应用。

工具栏提示与 PDF 专项回归：`python -u scripts/editor_export_qa.py --exe target/release/e-sticker.exe`。脚本在隔离数据目录中检查普通与最小窗口提示方向、保存对话框取消后重试，以及三种编辑模式实际生成 PDF；内容包含中文、任务列表、表格、公式、本地图片和长文分页。PDF 位于该次 `artifacts/export-qa-*` 目录，可打开检查排版或用 PDF 解析工具检查正文与页数。

```powershell
powershell -File scripts/package.ps1
Expand-Archive dist/E-Sticker-v0.3.1-windows-x64.zip artifacts/release-check-v0.3.1
python -u scripts/multiwall_qa.py --exe artifacts/release-check-v0.3.1/E-Sticker-v0.3.1-windows-x64/E-Sticker.exe --output artifacts/qa-multiwall-packaged-v0.3.1
python -u scripts/editor_qa.py --exe artifacts/release-check-v0.3.1/E-Sticker-v0.3.1-windows-x64/E-Sticker.exe --output artifacts/qa-packaged-v0.3.1
python -u scripts/interaction_qa.py --exe artifacts/release-check-v0.3.1/E-Sticker-v0.3.1-windows-x64/E-Sticker.exe --output artifacts/qa-interaction-packaged-v0.3.1
python -u scripts/editor_export_qa.py --exe artifacts/release-check-v0.3.1/E-Sticker-v0.3.1-windows-x64/E-Sticker.exe
```

必须测试压缩包实际解压出的 EXE，而不仅是 debug 产物。发布包附带 `SHA256SUMS.txt`；ZIP 同目录另有 `.sha256` 校验文件。升级不要混用旧版 EXE。
