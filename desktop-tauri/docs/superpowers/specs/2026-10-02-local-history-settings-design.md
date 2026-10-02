# ClipSync Local History Settings Design

## Goal

Rename the user-facing application to ClipSync and let users configure how many clipboard entries are retained and where those entries are stored locally.

## Main Window

- Replace the visible ClipBridge name with ClipSync.
- Remove the sentence about minimizing the application and pressing `Ctrl+Alt+Z`.
- Keep the existing server connection controls.
- Add a history settings section containing:
  - A numeric history limit input accepting values from 1 through 1000, defaulting to 10.
  - A read-only field showing the current storage directory.
  - A button that opens the native Windows folder picker.
  - A save button and status text for success or errors.
- Load the persisted settings when the main window initializes.

## Application Naming

- Change the main heading, main window title, history window title, and Tauri bundle product name to ClipSync.
- Keep the identifier `com.clipbridge.desktop` so existing application data remains discoverable after an upgrade.
- Internal Rust crate and JavaScript package names may remain unchanged because they are implementation identifiers, not user-facing names.

## Settings Storage

- Store settings in `settings.json` under Tauri's application configuration directory.
- The settings document contains a history limit and history directory.
- If no settings file exists, use a limit of 10 and the application data directory as the history directory.
- Reject limits outside 1 through 1000.
- Return settings to the frontend through a Tauri command.

## History Storage

- Store clipboard entries as a JSON string array in `<selected-directory>/clipsync-history.json`.
- Load this file during application startup before the windows begin normal operation.
- Preserve newest-first ordering and duplicate removal.
- Persist history after local copies, remote clipboard messages, and history-item selections change the ordering.
- Apply the configured limit whenever an entry is added, settings are saved, or history is loaded.
- A missing history file represents an empty history and is not an error.
- Invalid JSON or filesystem errors are surfaced rather than silently replacing existing data.

## Directory Migration

- The frontend uses Tauri's native dialog plugin to select a directory.
- Saving an unchanged directory updates only the limit and trims history if necessary.
- Saving a new directory writes the current in-memory history to the new directory first.
- Only after the new file and settings file are written successfully does the application switch to the new path.
- After a successful switch, remove the old history file. Failure to remove the old file does not roll back the successful migration or lose data.
- Do not merge an unrelated history file already present in the destination; replace it with the current history after explicit user confirmation through the save action.

## Runtime State And Events

- Rust remains the source of truth for history, limit, and storage paths.
- Existing `clipboard-history-changed` events continue to refresh a visible pinned history window.
- Emit a history change after reducing the limit if entries were removed.
- Return command errors as readable strings for display in the main window.

## Verification

- Unit-test history limit validation, truncation, settings defaults, JSON loading, persistence, and directory migration behavior.
- Test that the main window exposes the count and directory controls and no longer contains the removed instruction.
- Run frontend tests, the TypeScript/Vite production build, and the complete Rust test suite.
