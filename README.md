# E-Sticker

**像纸一样贴，像文章一样找。** E-Sticker 是一款轻量的 Windows 本地电子便签。先在纸上写一句，亲手贴到墙上；想记更多时，再打开同一张便签补写 Markdown。

## 构建和运行

在 Windows 上使用 Rust stable 和 MSVC 工具链构建（需安装 Visual Studio Build Tools 的「使用 C++ 的桌面开发」组件）。在仓库根目录运行：

```powershell
cargo build --release --locked
.\target\release\e-sticker.exe
```

生成的 EXE 可以单独复制到固定目录，也可以用 uTools 启动。应用用 Rust + Win32 编写，不内置浏览器运行时，无需登录，关窗即退出。

## 使用方法

1. 点「新建便签」，直接在纸上写，选择纸色。按 `Enter` 换行；内容超出纸面时会提示，不会默默截断。
2. 点「去贴上」，把纸移到想贴的位置，再单击落贴。可以盖住别的便签；按 `Esc` 返回编辑，草稿仍在。
3. 单击便签查看详情、补写 Markdown，或查看创建与编辑时间。拖动便签可以揭开下面的纸，移走的纸会放到最上层。
4. 切换「文章列表」可按最近编辑、创建时间或短句排序。搜索同时匹配短句和正文，也能找到被完全盖住的便签。排序不改动墙面布局。

| 操作 | 快捷键 |
| --- | --- |
| 新建便签 | `Ctrl+N` |
| 搜索 | `Ctrl+F` |
| 确认输入 | `Ctrl+Enter` |
| 取消落贴、返回编辑 | `Esc` |
| 横向移动墙面 | `Shift+滚轮` |

## 本地数据和备份

- 新安装把便签内容、位置和叠放顺序保存到 `%LOCALAPPDATA%\E-Sticker\wall.json`，不会上传内容。备份前先关闭应用，再复制该文件。
- 如果新版目录没有数据、但早期原型的 `%LOCALAPPDATA%\Paperwall\wall.json` 存在，E-Sticker 会继续读写原文件，无需手动迁移。
- 想隔离试用数据，可在启动前将 `ESTICKER_DATA_DIR` 设为另一个目录。

## v0.1.0 范围

首版包含五种纸色、一面固定大小的可滚动墙，以及标题、无序列表、引用和围栏代码块的基础 Markdown 预览。自定义纸样、扩墙和换墙、桌面挂件暂未实现。

项目术语见 [CONTEXT.md](CONTEXT.md)，设计取舍见 [决策记录](docs/adr)。
