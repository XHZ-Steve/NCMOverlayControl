# NCM Overlay Control

为 Steam Overlay 等小窗口场景设计的本地网易云音乐控件。

[下载 Windows x64 便携版](https://github.com/XHZ-Steve/NCMOverlayControl/releases/latest)

## 使用

1. 下载并运行 `NCMOverlayControl.exe`，程序驻留托盘，默认用浏览器打开设置，不显示控制台或应用窗口；可关闭“启动应用时显示设置界面”。
2. 左键托盘图标，在默认浏览器打开设置；填写或查找网易云路径，点击“以调试模式启动网易云”。重启可能中断当前播放。
3. 复制设置页上的地址，在游戏内按 `Shift + Tab`，用 Steam Overlay 浏览器打开。

右键托盘图标可选择“关于”或“退出”。“关于”打开 [软件 的 GitHub 主页](https://github.com/XHZ-Steve/NCMOverlayControl)。关闭网页不会退出后台程序，退出控件也不会退出网易云。

## 功能

- 上一首、播放/暂停、下一首；Windows 媒体键与网易云直接控制可切换。
- Windows 媒体键未响应时，可由网易云控制兜底一次。
- 喜欢、原生不喜欢/减少推荐选项、播放模式、心动模式、私人漫游、音量。
- 自定义网页和网易云调试端口，设置实时应用；端口冲突时保留原服务。
- “始终使用调试模式启动网易云”在控件运行期间生效。检测到指定路径的网易云以普通模式启动，会自动重启为调试模式，可能中断播放；关闭控件后不改变网易云的启动方式。未开启时不监测进程。
- 已正常连接调试模式的网易云时，禁用调试启动按钮；断连或选择不同调试端口后可重新启动。
- 播放页断连且检测到网易云进程时，可点击“尝试以调试模式启动网易云”；需确认后才会重启。未填写路径时从正在运行的唯一网易云路径识别，多路径时需先在设置中选择。
- 无安装、无运行时下载、无外置网页文件。发布文件只有一个 exe。
- 设置保存在 `HKEY_CURRENT_USER\Software\NCMOverlayControl`，不生成配置文件。不同 Windows 用户的设置互相独立。

默认网页地址为 `http://127.0.0.1:17890/`，调试端口为 `9222`。启动时网页端口被占用，会改用可用端口，请以托盘设置页显示的地址为准。重复运行只打开已有实例的设置页。

## 要求与边界

- Windows 10/11 x64、网易云音乐 3.x、系统自带 Windows PowerShell。
- 网易云需要启用本地调试端口；可使用网页中的连接按钮，或自行以 `--remote-debugging-port=9222 --remote-debugging-address=127.0.0.1` 启动。
- 调试接口仅用于本机。不要开放至局域网或公网。Windows 媒体键由系统分配，其他播放器也可能响应。
- 不喜欢选项使用网易云的原生流程；打开/取消不会提交。某些项目需要在网易云中进一步确认。
- 当前适配网易云 3.1.40。客户端更新可能改变内部入口；失败时不会自动重放切歌或反馈操作。
- 程序未签名，首次运行可能出现系统或安全软件提示，请核对 Release 的 SHA-256；不要关闭安全防护。

## 构建

```powershell
cargo build --release --locked
```

输出：`target\release\ncm-overlay-control.exe`。静态链接运行库，仅依赖 Windows 系统组件。依赖许可证内嵌于 exe，可从设置页查看。

测试：

```powershell
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

交互式 Windows 桌面还可运行 `cargo test native_tray_lifecycle -- --ignored --test-threads=1`，检查真实托盘图标注册与退出清理。

## 开发约定

界面保持简洁，只保留必要标签、错误、风险及操作结果；详细说明放在文档中。所有发布文件必须保持单 exe，不写入用户配置文件，不依赖外部服务。

MIT License · XHZ_Steve
