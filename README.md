# ctxmenu — Windows 10 右键菜单管理器 / Context Menu Manager

Rust + egui 编写的单文件 GUI 工具，管理资源管理器右键菜单。界面中英双语（顶栏切换，选择保存在 exe 旁的 `ctxmenu.ini`）。

A single-file GUI utility (Rust + egui) for managing the Windows 10 Explorer context menu and the "Open with" list. Bilingual UI (中文/English toggle in the top bar).

## 功能

- **扫描**：遍历 HKLM / HKCU 下 `Software\Classes` 的 6 个位置（所有文件、文件+文件夹、文件夹、背景/桌面、文件夹(通用)、驱动器），同时覆盖两类菜单项：
  - `shell\<verb>` 传统菜单命令
  - `shellex\ContextMenuHandlers` 第三方 COM 扩展（压缩工具、网盘、杀毒等）
- **启用/禁用**（可逆，不删注册表项）：
  - 菜单命令：写入/移除 `LegacyDisable` 值
  - Shell 扩展：把 CLSID 加入/移出 `Shell Extensions\Blocked` 列表（禁用写 HKCU，按 CLSID 全局生效）
- **删除**：删除前自动导出 `.reg` 备份到 exe 旁的 `ctxmenu_backups\deleted\`，双击备份文件即可恢复
- **添加自定义菜单项**：写入 HKCU（仅当前用户），支持图标和"仅 Shift 显示"；键名带 `ctxmenu.` 前缀便于识别
- **全量备份**：一键把所有扫描位置导出为 `.reg` 到 `ctxmenu_backups\full_<时间戳>\`
- **"打开方式"列表管理**（独立页签）：按扩展名扫描 "Open with" 子菜单的全部来源，支持一次输入多个扩展名（空格/逗号分隔，内置"常用文本类型"预设）——
  - `HKCU\...\Explorer\FileExts\<ext>\OpenWithList`（最近使用 MRU，删值并同步修正 MRUList）
  - `FileExts\<ext>\OpenWithProgids` 及 HKLM/HKCU `Software\Classes\<ext>\OpenWithProgids`
  - `Software\Classes\<ext>\OpenWithList`、`SystemFileAssociations\<ext>` 与感知类型（如 text）
  - `Applications\<exe>\SupportedTypes` 应用自注册
  - 支持逐条**移除**（先自动备份父键）或整应用**隐藏**（写 `NoOpenWith` 标记，可一键取消，无需删注册表）
  - VS Code 系应用（Antigravity/Kiro/CodeBuddy 等）按扩展名批量注册 ProgID（`Antigravity.txt`、`Antigravity.md`…），"隐藏应用"会自动找出同前缀的**整族 ProgID** 一起标记，从所有文件类型的"打开方式"里消失
- **重启资源管理器**按钮：让改动立即生效

## 下载 / Download

单文件绿色版：从 [Releases](../../releases) 页面下载 `ctxmenu-*-windows-x64.exe`，放到任意可写目录直接运行（无需安装，启动时会请求 UAC 提权）。程序会在 exe 旁生成 `ctxmenu.ini`（语言设置）和 `ctxmenu_backups\`（注册表备份）。

Portable single file: grab `ctxmenu-*-windows-x64.exe` from [Releases](../../releases) and run it from any writable folder. No installation or runtime required (Windows 10/11).

## 构建 / Build

```powershell
cargo build --release        # 产物: target\release\ctxmenu.exe（约 3 MB，启动时请求 UAC 提权）
cargo test --no-default-features   # 跑测试（关闭管理员清单，否则测试进程无法启动）
```

发布新版本：推送 `v*` 标签（如 `git tag v0.1.0 && git push origin v0.1.0`），GitHub Actions 会自动构建并把 exe 挂到 Release 页面（见 `.github/workflows/release.yml`）。

## 许可 / License

[MIT](LICENSE)

## 注意

- exe 内嵌 `requireAdministrator` 清单，每次启动弹 UAC——修改 HKLM 项必需
- 禁用 Shell 扩展按 CLSID 生效：同一扩展出现在多个位置时会一起禁用
- 部分显示名是 MUI 间接字符串（如 `@shell32.dll,-8506`），按原样显示
- 禁用/删除后新开的资源管理器窗口才会生效；不生效时可重启 explorer
