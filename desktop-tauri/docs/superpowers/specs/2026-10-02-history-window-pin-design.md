# History Window Pin Design

## Goal

Make the clipboard history window dismiss itself during normal use while allowing the user to keep it open when needed.

## Interaction

- Add a window pin button to the history window header, next to the close button.
- The unpinned state is the default.
- When unpinned, selecting a history item copies and pastes it, then hides the history window.
- When unpinned, moving focus to another application or window hides the history window.
- When pinned, selecting a history item still copies and pastes it, but the history window remains visible.
- When pinned, losing focus does not hide the history window.
- Clicking the pin button again returns to the unpinned behavior.
- The close button always hides the window, regardless of pin state.
- Persist the pin state in `localStorage` so it survives application restarts.
- Keep the existing per-record star action unchanged; it continues to sort selected records above other history entries.

## Implementation

- Keep the state and focus handling in `src/main.ts`, matching the existing frontend-owned opacity and per-record pin settings.
- Extract the automatic-hide decision into a small pure function so the behavior can be tested without a Tauri runtime.
- Subscribe to the current Tauri window's focus changes. Hide only when focus is lost and the window is not pinned.
- After `select_clipboard_history` finishes, hide only when the window is not pinned.
- Update the pin button's accessible label, pressed state, title, and visual active state whenever the setting changes.
- Add focused CSS for the two header controls without changing the current compact transparent-window design.

## Error Handling

- Treat any persisted value other than the explicit string `true` as unpinned.
- Use the existing asynchronous Tauri calls. A failed copy or paste must not be reported as a successful selection.
- Register focus handling once when the history window initializes.

## Verification

- Add frontend unit tests for the automatic-hide decision in pinned and unpinned states.
- Run the frontend tests, TypeScript/Vite production build, and existing Rust test suite.
- Manually verify both pin states in the Tauri application when a desktop session is available.
