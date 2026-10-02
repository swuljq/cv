# ClipSync for Ubuntu

ClipSync is a Tauri 2 clipboard synchronization client for Ubuntu 22.04 or newer on x86_64. It targets X11 so that `Ctrl+Alt+Z` can open clipboard history near the pointer and paste a selected entry back into the previously active application.

## Supported Environment

- Ubuntu 22.04 or newer
- x86_64 CPU
- X11 desktop session
- Debian package or AppImage

Wayland is not supported because it blocks reliable global focus restoration and synthetic `Ctrl+V` input. At the login screen, choose an X11 session such as **Ubuntu on Xorg**.

## Build Prerequisites

Install the Ubuntu development packages used by Tauri, WebKitGTK, TLS, and X11 input handling:

```bash
sudo apt update
sudo apt install build-essential curl file libayatana-appindicator3-dev libdbus-1-dev libssl-dev libwebkit2gtk-4.1-dev libxdo-dev libxkbcommon-dev librsvg2-dev patchelf pkg-config wget
```

Install Node.js 20 or newer and the Rust stable toolchain, then install JavaScript dependencies:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
npm install
```

## Development

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- dev
```

The desktop session must expose `XDG_SESSION_TYPE=x11` and a valid `DISPLAY` value.

## Build Ubuntu Packages

Build both distribution formats on Ubuntu 22.04 to preserve compatibility with that glibc baseline:

```bash
npm run tauri:build:ubuntu
```

Artifacts are written below:

```text
src-tauri/target/release/bundle/deb/
src-tauri/target/release/bundle/appimage/
```

Install the Debian package with `sudo apt install ./path/to/clipsync.deb`. Make the AppImage executable with `chmod +x ./ClipSync.AppImage`, then launch it directly.
