# Cross-Platform Image Clipboard Sync Design

## Scope

Add image clipboard synchronization to the existing ClipSync protocol and history model.

- Implement the Windows client in `desktop-tauri` first.
- The Ubuntu client implements the same wire protocol and storage schema in its separate project directory.
- Keep the current text synchronization behavior unchanged.
- Do not create installers as part of this feature unless explicitly requested later.

## Supported Sources

The Windows client accepts images from both clipboard representations below:

1. Image content copied from screenshots, browsers, image editors, and other applications.
2. Image files copied from Windows File Explorer.

Supported source file formats are PNG, JPEG/JPG, WebP, BMP, GIF, and TIFF. Source formats are decoded and normalized to PNG before synchronization. GIF input uses only the first frame; animation is not preserved.

When a file-list clipboard contains multiple images, synchronize only the first supported image. Ignore non-image files. A copied image file is restored on receiving devices as image content, not as a file-list clipboard item.

The Ubuntu implementation should support image content from its native clipboard. File-manager image-file detection is optional unless the Ubuntu project explicitly requires it, but any image it sends must follow the same normalized protocol.

## Wire Protocol

The existing WebSocket message envelope remains unchanged. No server source change is required because the server already accepts and forwards `contentType: "image"` messages.

Sender message:

```json
{
  "type": "clipboard",
  "eventId": "desktop-uuid:event-uuid",
  "contentType": "image",
  "data": "<base64 encoded PNG bytes>"
}
```

Forwarded server message:

```json
{
  "type": "clipboard",
  "eventId": "desktop-uuid:event-uuid",
  "contentType": "image",
  "data": "<base64 encoded PNG bytes>",
  "sourceDeviceId": "desktop-uuid",
  "createdAt": "2026-10-02T00:00:00.000Z"
}
```

Protocol rules:

- `data` contains standard Base64 without a data-URL prefix or whitespace.
- Decoded bytes must be a valid PNG file.
- The maximum decoded PNG size is 10 MiB (`10 * 1024 * 1024` bytes).
- Reject invalid Base64, invalid PNG data, dimensions greater than 16,384 pixels on either axis, or more than 40 million decoded pixels.
- The Base64 transport representation may be larger than 10 MiB due to encoding overhead; limits apply to decoded PNG bytes.
- Generate a new event UUID for every local clipboard change.
- Preserve the sender's persistent device UUID in the event ID prefix for diagnostics; uniqueness must not depend only on timestamps.

## Encoding And Loop Prevention

The sending client performs these steps:

1. Read clipboard image pixels or the first supported copied image file.
2. Decode to RGBA pixels.
3. Validate dimensions and pixel count.
4. Encode a canonical PNG.
5. Reject the image if the encoded PNG exceeds 10 MiB.
6. Compute a SHA-256 fingerprint of the PNG bytes.
7. Skip transmission when the fingerprint matches the most recently processed clipboard image.
8. Base64-encode the PNG and send the image message.

The receiving client validates and decodes the payload before changing the system clipboard. It records the received fingerprint before writing the image to the clipboard so clipboard polling does not send the same image back to the server.

Text and image fingerprints are tracked separately. A clipboard representation change from text to image, or image to text, is always treated as a new local event.

## Local History Schema

Text and image entries share one newest-first history list and one configured item limit. For example, a limit of 10 stores the latest 10 combined text and image entries.

Replace the legacy string-array history file with this versioned structure:

```json
{
  "version": 2,
  "entries": [
    {
      "id": "entry-uuid",
      "kind": "text",
      "text": "example",
      "createdAt": "2026-10-02T00:00:00.000Z"
    },
    {
      "id": "entry-uuid",
      "kind": "image",
      "fileName": "entry-uuid.png",
      "thumbnailName": "entry-uuid.png",
      "sha256": "hex-encoded-sha256",
      "byteLength": 123456,
      "width": 1920,
      "height": 1080,
      "createdAt": "2026-10-02T00:00:00.000Z"
    }
  ]
}
```

Storage layout under the selected history directory:

```text
clipsync-history.json
clipsync-images/
  <entry-id>.png
clipsync-thumbnails/
  <entry-id>.png
```

File names in metadata are base names only, never absolute paths or parent-relative paths. Clients resolve them only inside the fixed image and thumbnail directories.

## Legacy Migration

On startup, detect the existing JSON string array and migrate it to schema version 2.

- Preserve the original newest-first text order.
- Generate an ID and `createdAt` value for every migrated text entry.
- Do not change the configured history limit or storage directory.
- Write the version 2 file using the existing temporary-file and backup recovery mechanism.
- If migration fails, keep the original file and surface an error instead of starting with empty history.

Both Windows and Ubuntu clients must read schema version 2. Only the Windows client is required to perform migration from the current legacy format because it owns the existing data.

## Image Files And Thumbnails

- Store the full normalized PNG in `clipsync-images`.
- Generate a PNG thumbnail bounded to 320 by 180 pixels while preserving aspect ratio.
- Never upscale small images.
- The history list command returns entry metadata but not full image bytes.
- Load thumbnails through a dedicated command by entry ID and only when an image row approaches the visible scroll area.
- Validate entry IDs before resolving paths.
- A missing thumbnail may be regenerated from the full PNG.
- A missing full PNG marks the entry unavailable; the UI should show a clear placeholder and must not crash.

## Duplicate And Limit Behavior

- Text duplicates use exact string equality.
- Image duplicates use the SHA-256 fingerprint of normalized PNG bytes.
- Repeating an existing entry moves it to the top instead of creating another entry.
- Trimming applies to the combined text/image list.
- After the new metadata file is safely persisted, delete full images and thumbnails belonging to trimmed image entries.
- Orphan cleanup must only remove files whose names are valid entry UUID PNG names and are not referenced by current metadata.
- Directory migration copies the metadata file, full images, and thumbnails before switching settings. If any required copy fails, retain the previous directory and history.

## Clipboard And Selection Behavior

Receiving a remote image writes image pixels to the local system clipboard and adds the image to local history.

Selecting an image history entry:

- Windows: restore image pixels to the clipboard, restore the previous target window, and use the existing simulated `Ctrl+V` behavior. Respect the current pinned/unpinned window behavior.
- Ubuntu: restore image pixels to the clipboard only. The user manually presses `Ctrl+V`, avoiding unreliable Wayland input simulation.

Selecting a text entry remains unchanged on each platform.

## Error Handling

- Oversized local images are not sent or added to history.
- Invalid or oversized remote images are ignored and must not overwrite the current clipboard.
- Emit a user-visible synchronization warning event for rejected local or remote images when the main window is available.
- Disk failures must roll back in-memory history changes.
- A failed thumbnail write must not discard a successfully saved full image; retain the history entry and regenerate the thumbnail later.
- Never delete an existing history image until replacement metadata is safely persisted.

## Windows Implementation Boundaries

- Use `arboard` for ordinary image clipboard content.
- Use Windows clipboard file-list APIs (`CF_HDROP` and `DragQueryFileW`) for File Explorer image files.
- Place Windows-specific imports and dependencies behind `cfg(target_os = "windows")` where practical so the protocol and storage logic remain reusable.
- Use image-decoding code that explicitly enables only the required source formats.

## Verification

Windows automated coverage must include:

- PNG encode/decode round trips.
- JPG, WebP, BMP, GIF first-frame, and TIFF source decoding.
- 10 MiB encoded PNG acceptance and rejection boundaries.
- Invalid Base64, invalid PNG, excessive dimensions, and excessive pixel count.
- Image fingerprint duplicate suppression and loop prevention.
- Text and image combined ordering and limit trimming.
- Legacy text-history migration to version 2.
- Full image and thumbnail persistence.
- Trimmed-image cleanup and failed-write rollback.
- Directory migration including images and thumbnails.
- History metadata responses and thumbnail loading.
- Image selection restoring clipboard content.
- Existing text synchronization regression tests.

Ubuntu verification must use the same protocol fixtures for Base64 decoding, PNG validation, size limits, fingerprints, and schema version 2 compatibility.

Manual cross-platform verification:

1. Copy a screenshot on Windows and paste it on Ubuntu.
2. Copy an image on Ubuntu and paste it on Windows.
3. Copy JPG, PNG, WebP, BMP, GIF, and TIFF files from Windows File Explorer and confirm the first image becomes pasteable image content on Ubuntu.
4. Confirm images appear as thumbnails in both history windows.
5. Confirm pinned history windows update when remote images arrive.
6. Confirm images above 10 MiB are rejected without changing the remote clipboard.
7. Confirm repeated images do not bounce indefinitely between devices.
