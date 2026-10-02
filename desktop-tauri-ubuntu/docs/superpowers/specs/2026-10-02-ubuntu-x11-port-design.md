# ClipSync Ubuntu X11 Port Design

## Goal

Port the existing Windows-oriented ClipSync desktop application to Ubuntu 22.04 or newer on x86_64. The application will target X11, preserve the global history shortcut and automatic paste workflow, and produce Debian and AppImage packages. Windows builds are no longer in scope.

## Supported Environment

- Ubuntu 22.04 or newer on x86_64.
- An X11 desktop session with the `DISPLAY` environment variable available.
- Debian (`.deb`) and AppImage distribution formats.
- Wayland is not a supported automatic-paste environment. Startup must report a clear session error instead of silently providing partially working behavior.

## Desktop Integration

- Remove the `windows` crate and all direct Win32 API calls.
- Add `x11rb` as the direct X11 dependency.
- Put Ubuntu/X11-specific behavior in `src-tauri/src/linux_desktop.rs` so history and WebSocket logic remain platform-independent.
- The desktop module exposes operations to:
  - Validate that the process is running in an X11 session.
  - Read the currently active X11 window.
  - Read the pointer position used to place the history window.
  - Restore focus to the previously active window.
  - Send `Ctrl+V` through the existing `enigo` dependency.
- Store the target X11 window identifier when the global shortcut is pressed.
- Center the history window around the pointer and constrain it to the monitor work area using the existing positioning calculation.
- After a history item is selected, write it to the clipboard, restore the target window, wait briefly for focus to settle, and send `Ctrl+V`.

## Runtime Behavior

- Keep `CommandOrControl+Alt+Z` as the global history shortcut. On Ubuntu this resolves to `Ctrl+Alt+Z`.
- Keep the existing history, pinning, transparency, settings, WebSocket synchronization, and persistence behavior.
- Identify the WebSocket client as `ubuntu-tauri` with the display name `Ubuntu desktop`.
- Clipboard data remains available for manual paste if focus restoration or synthetic input fails.
- X11 failures return readable errors through the existing Tauri command boundary; they must not panic the application.
- If pointer lookup fails while opening history, show the history window using a safe fallback position.
- Window show and focus failures should be logged without terminating the background synchronization thread.

## Packaging

- Limit Tauri bundle targets to `deb` and `appimage`.
- Add an npm script that builds both Linux bundle formats explicitly.
- Update project metadata and README text to describe ClipSync as an Ubuntu X11 application.
- Document Ubuntu build prerequisites, including WebKitGTK 4.1, GTK, AppIndicator, SVG, compiler, OpenSSL, and AppImage packaging tools required by Tauri.
- Document output paths under `src-tauri/target/release/bundle/` and installation or launch commands for both package formats.
- Use Ubuntu 22.04 as the oldest build baseline so generated binaries do not accidentally require a newer glibc version.

## Error Handling

- Session validation distinguishes an unsupported Wayland session from a missing X11 display.
- X11 connection, atom lookup, property parsing, focus restoration, and input simulation errors include operation context.
- Selecting a history item succeeds only when clipboard update and requested automatic paste complete. Failures remain visible to the frontend rather than being treated as successful selections.
- Existing clipboard history persistence rollback behavior remains unchanged.

## Verification

- Unit-test X11 session classification without requiring a live desktop connection.
- Unit-test Ubuntu WebSocket device identity.
- Keep and run existing history limit, persistence, migration, shortcut, and positioning tests.
- Run `npm test` and `npm run build`.
- Run `cargo test` and `cargo check` for the Tauri crate.
- When Ubuntu development packages are installed, run `npm run tauri:build:ubuntu` and confirm that both `.deb` and `.AppImage` files are produced.
- Manually verify on X11 that `Ctrl+Alt+Z` opens the history window near the pointer and selecting an item pastes into the previously active application.

## Out Of Scope

- Windows and macOS builds.
- ARM packages.
- Full Wayland automatic paste support.
- Changes to the WebSocket protocol beyond the Ubuntu device identity.
- Unrelated connection lifecycle, credential storage, CSP, or frontend refactoring.
