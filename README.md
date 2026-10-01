# ClipBridge MVP

当前版本用于先验证“同一账号、多设备、文本/图片剪贴板同步”的核心链路。

## 本地测试

```powershell
npm install
npm run server
```

然后运行 `desktop/dist/win-unpacked/ClipBridge.exe`。打开两个客户端，使用默认账号登录：

- 账号：`clipbridge`
- 密码：`clipbridge-dev`
- 服务地址：`ws://127.0.0.1:8787`

在任意客户端复制文本或图片，另一个客户端会收到并写入系统剪贴板。

## 构建 Windows 可执行文件

```powershell
npm install
npm run package:win
```

构建产物位于 `desktop/dist/`。当前构建使用 Electron，后续会根据轻量化目标替换为更轻的运行时。

## 当前范围

- 已实现：Node WebSocket 中转服务、固定账号、Windows 桌面端、文本和 PNG 图片同步、回环抑制。
- 下一步：服务端远程部署配置、HTTPS/WSS、Android Capacitor 客户端、Linux 打包、本地历史和安全存储。
