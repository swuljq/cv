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

手机连接电脑上的服务时，在 PowerShell 中使用局域网监听：

```powershell
$env:HOST="0.0.0.0"
$env:PORT="8787"
npm run server
ipconfig
```

把手机端服务器地址填写为 `ws://电脑的IPv4地址:8787`，例如 `ws://192.168.1.20:8787`。电脑和手机必须连接同一个局域网；如果 Windows 防火墙弹出提示，需要允许 Node.js 通过专用网络通信。

## 构建 Windows 可执行文件

```powershell
npm install
npm run package:win
```

构建产物位于 `desktop/dist/`。当前构建使用 Electron，后续会根据轻量化目标替换为更轻的运行时。

## Android APK

Android 工程已经生成在 `mobile/android/`，同步前端资源：

```powershell
npm run android:sync
```

然后在安装了 Android SDK 的机器上构建：

```powershell
./mobile/android/gradlew.bat -p mobile/android assembleDebug
```

APK 会输出到 `mobile/android/app/build/outputs/apk/debug/`。当前开发环境已生成 Android 工程，但没有配置 Android SDK，因此还不能在这里完成 APK 编译。

## 当前范围

- 已实现：Node WebSocket 中转服务、固定账号、Windows 桌面端、文本和 PNG 图片同步、回环抑制。
- 已生成：Android Capacitor 客户端工程，支持前台文本剪贴板同步。
- 下一步：服务端远程部署配置、HTTPS/WSS、Android 后台剪贴板策略、图片同步、Linux 打包、本地历史和安全存储。
