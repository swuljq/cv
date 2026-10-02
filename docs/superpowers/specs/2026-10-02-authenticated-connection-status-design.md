# Authenticated Connection Status Design

## Goal

ClipSync must display a successful connection only after the server explicitly accepts the supplied username and password. Starting a background thread or opening a TCP/WebSocket connection is not sufficient evidence that synchronization is active.

This behavior must be implemented consistently in both the Windows and Ubuntu clients.

This change intentionally fixes the accuracy of the initial connection result. Live “reconnecting/offline” UI updates and a user-triggered stop command are separate future features and are out of scope.

## Client State Model

Use these logical states even if the implementation stores them as one atomic running flag plus thread ownership:

| State | Meaning | Start request |
|---|---|---|
| `Idle` | No connection attempt or background task owns synchronization | Allowed |
| `Authenticating` | One caller reserved synchronization and is performing the initial handshake | Rejected |
| `Running` | Initial authentication succeeded and one background task owns the authenticated socket or its reconnect loop | Rejected |

The existing atomic running flag represents both `Authenticating` and `Running`:

- A successful compare-and-swap from `false` to `true` enters `Authenticating`.
- Returning an initial error resets it to `false` and returns to `Idle`.
- Successfully spawning the background task transitions logically to `Running`; the flag stays `true`.
- There is no stop transition in this feature.

## Existing Server Protocol

After a WebSocket connection opens, the client sends:

```json
{
  "type": "auth",
  "username": "clipsync",
  "password": "user-supplied password",
  "deviceId": "persistent-device-uuid",
  "deviceName": "ClipSync desktop"
}
```

Successful authentication returns:

```json
{
  "type": "auth_ok",
  "deviceId": "persistent-device-uuid"
}
```

Failed authentication returns:

```json
{
  "type": "error",
  "code": "AUTH_FAILED"
}
```

The server then closes the rejected connection.

## Required Initial Connection Flow

The `start_sync` command must follow this order:

1. Reject the request if a synchronization session is already running.
2. Reserve the synchronization state so two connection requests cannot start concurrently.
3. Open the WebSocket connection.
4. Apply a finite authentication read timeout. Use 5 seconds unless the platform requires a comparable value.
5. Send the authentication message.
6. Read exactly the first authentication response before reporting success.
7. Accept only a text JSON message whose `type` is `auth_ok`.
8. On success, switch to the normal short polling/read timeout and start the background synchronization loop.
9. On any connection, send, timeout, parse, protocol, or authentication error, release the reserved synchronization state and return an error to the UI.

The 5-second timeout applies to waiting for the authentication response after the WebSocket connection has opened. Connection establishment uses the WebSocket library's normal connection behavior. A close frame or socket close before `auth_ok` is an authentication read failure.

The frontend changes the status to “connected” only after `start_sync` returns success.

## Error Mapping

Clients should return readable messages instead of raw protocol details where possible.

| Condition | User-facing result |
|---|---|
| `AUTH_FAILED` | `账号或密码错误` |
| WebSocket connection failure | `无法连接服务器：<reason>` |
| Authentication send failure | `发送认证信息失败：<reason>` |
| Authentication response timeout/read failure | `读取认证结果失败：<reason>` |
| Non-text first response | `服务器未返回认证结果` |
| Invalid JSON response | `服务器认证响应无效` |
| Response type other than `auth_ok` or `error` | `服务器未确认认证成功` |
| Another session is already active | `同步已经在运行` |

The frontend should continue to display these errors through its existing `连接失败：...` status.

## Reconnection Flow

After an authenticated session later disconnects:

- Keep the background synchronization task alive.
- Reconnect with exponential backoff, starting at 1 second and capped at 30 seconds.
- Every new WebSocket connection must repeat the complete authentication handshake and wait for `auth_ok` before sending or accepting clipboard messages.
- Never send clipboard payloads on an unauthenticated connection.
- Preserve the same persistent device ID across reconnects.
- Reset backoff to 1 second after every successful authenticated reconnect. Jitter is not required.
- Connection errors and reconnect authentication errors use the same 1-to-30-second backoff and continue retrying with the credentials captured by the successful initial `start_sync` call.
- A reconnect `AUTH_FAILED` does not process clipboard traffic and does not clear the running flag; it continues retrying because there is currently no API for replacing credentials in an existing background task.
- The current UI may still display the result of the previously successful initial session while the background task reconnects. This is a known limitation of the current UI, not evidence that the reconnect is authenticated. A future live-status event should add explicit `reconnecting` and `offline` states.

## State Rules

- Authentication failure during the initial connection resets the running flag to `false`, allowing the user to correct credentials and try again.
- Failure to create the background thread also resets the running flag.
- A successful initial authentication changes the running flag only once and starts one background task.
- Repeated clicks after success return `同步已经在运行`; they must not silently accept different credentials.
- The device ID remains part of the persisted local settings and is not regenerated during connection retries.
- Username and password are retained only in memory by the running background task. This feature does not persist credentials.

## Authentication Response Validation

- The first application data message must be a WebSocket text message containing a JSON object.
- The JSON object must contain a string `type` field.
- `type: "auth_ok"` is sufficient for compatibility with the current server. A returned `deviceId` may be logged for diagnostics but is not required and does not replace the locally persisted device ID.
- `type: "error"` is always a failed handshake. A string `code` of `AUTH_FAILED` maps to `账号或密码错误`; another string code maps to `服务器拒绝连接：<code>`; a missing or non-string code maps to `服务器拒绝连接`.
- A text message with any other type, including `clipboard`, maps to `服务器未确认认证成功` and its payload must be discarded.
- Invalid JSON, JSON that is not an object, a missing `type`, or a non-string `type` is rejected as an invalid or unconfirmed authentication response.
- A binary application message is rejected as `服务器未返回认证结果`.
- The current server does not send ping/pong frames before authentication. If the selected WebSocket library surfaces control frames through the same read API, implementations may answer ping and continue waiting within the same 5-second deadline; they must never interpret a control frame as authentication success.
- Close the rejected socket before returning the initial error. Dropping the socket is sufficient when the WebSocket library performs normal close cleanup.

## Platform Requirements

### Windows

- Use the shared `connect_authenticated` helper for the initial connection and every reconnect.
- Apply read timeouts to both plain TCP and native TLS streams.
- Keep the existing clipboard text/image synchronization loop unchanged after authentication succeeds.
- Transfer the already authenticated socket into the spawned background thread; do not discard it and reconnect immediately.

### Ubuntu

- Implement the same handshake and response validation before spawning or confirming the synchronization loop.
- Apply read timeouts using the Linux stream type used by the Ubuntu Tauri client.
- Do not copy the Windows stream matching code directly if the Ubuntu WebSocket/TLS stream variants differ.
- Keep Ubuntu clipboard behavior unchanged after authentication succeeds.
- Keep the existing Ubuntu Tauri command signature compatible with the frontend: `start_sync(url, username, password) -> Result<(), String>`.
- Store the running guard in the Ubuntu backend process, not in frontend JavaScript state.
- Transfer the authenticated socket into the background task using the ownership mechanism supported by the Ubuntu client's Rust WebSocket library.
- Refer to `2026-10-02-image-clipboard-sync-design.md` for clipboard payload formats after authentication.

## Security And Compatibility

- Do not log or include the supplied password in error messages.
- Do not store a newly entered password merely because authentication was attempted.
- Do not persist the password after authentication succeeds; retain it only in the running task for reconnects.
- Continue using the existing `auth`, `auth_ok`, and `AUTH_FAILED` messages; no server update is required.
- Unknown server error codes may be shown as `服务器拒绝连接：<code>`.
- The client must reject a clipboard message received before `auth_ok` during the initial handshake.
- This change fixes status accuracy but does not add transport encryption. Public deployments should still use `wss://`.

## Automated Verification

Both clients must test the pure authentication-response validator with:

1. A valid `auth_ok` text response succeeds.
2. `{"type":"error","code":"AUTH_FAILED"}` returns `账号或密码错误`.
3. A clipboard message before authentication is rejected.
4. Invalid JSON is rejected.
5. A non-text response is rejected.
6. Unknown error codes return a readable rejection.
7. Missing or non-string `type` is rejected.
8. A binary first response is rejected.

Connection-level tests should additionally verify:

1. Correct credentials cause the UI to show connected only after `auth_ok`.
2. Incorrect username or password shows connection failure.
3. Authentication timeout resets the running state and permits another attempt.
4. Failure to spawn the background task resets the running state.
5. An authenticated disconnect reconnects and authenticates again before clipboard traffic resumes.
6. A second start request while running is rejected.
7. A reconnect `AUTH_FAILED` does not process clipboard traffic and continues backoff without spawning a second task.

## Manual Cross-Platform Verification

1. Enter an incorrect username on Windows; confirm it shows `连接失败：账号或密码错误` and does not synchronize.
2. Correct the username and reconnect without restarting the application; confirm it succeeds.
3. Repeat the same checks on Ubuntu.
4. Connect both clients with valid credentials and confirm text and image synchronization still works.
5. Restart the server while both clients are connected; confirm both clients reconnect with their persistent device IDs.
6. Change the server password and restart it; confirm old credentials cannot establish a new authenticated connection.
