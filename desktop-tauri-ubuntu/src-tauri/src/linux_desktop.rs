use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::{env, thread, time::Duration};
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window},
    rust_connection::RustConnection,
    CURRENT_TIME,
};

const ACTIVE_WINDOW_ATOM: &[u8] = b"_NET_ACTIVE_WINDOW";
const FOCUS_RETRY_DELAY: Duration = Duration::from_millis(25);
const FOCUS_RETRY_COUNT: usize = 12;

pub(crate) type WindowId = Window;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PointerPosition {
    pub x: i32,
    pub y: i32,
}

pub(crate) fn target_window_after_shortcut(
    previous: WindowId,
    history_focused: bool,
    active: Option<WindowId>,
) -> WindowId {
    if history_focused {
        previous
    } else {
        active.unwrap_or(0)
    }
}

pub(crate) fn validate_session(
    session_type: Option<&str>,
    display: Option<&str>,
) -> Result<(), String> {
    if session_type.is_some_and(|value| value.eq_ignore_ascii_case("wayland")) {
        return Err("ClipSync requires an Ubuntu X11 session; Wayland is not supported".into());
    }
    if display.is_none_or(|value| value.trim().is_empty()) {
        return Err("ClipSync requires an X11 display, but DISPLAY is not set".into());
    }
    Ok(())
}

fn connect() -> Result<(RustConnection, usize), String> {
    let session_type = env::var("XDG_SESSION_TYPE").ok();
    let display = env::var("DISPLAY").ok();
    validate_session(session_type.as_deref(), display.as_deref())?;
    x11rb::connect(None).map_err(|error| format!("failed to connect to X11: {error}"))
}

fn root_window(connection: &RustConnection, screen_number: usize) -> Result<Window, String> {
    connection
        .setup()
        .roots
        .get(screen_number)
        .map(|screen| screen.root)
        .ok_or_else(|| format!("X11 screen {screen_number} was not found"))
}

fn intern_active_window_atom(connection: &RustConnection) -> Result<u32, String> {
    connection
        .intern_atom(false, ACTIVE_WINDOW_ATOM)
        .map_err(|error| format!("failed to request the X11 active-window atom: {error}"))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| format!("failed to read the X11 active-window atom: {error}"))
}

pub(crate) fn ensure_x11_session() -> Result<(), String> {
    connect().map(|_| ())
}

pub(crate) fn active_window() -> Result<WindowId, String> {
    let (connection, screen_number) = connect()?;
    let root = root_window(&connection, screen_number)?;
    let active_window_atom = intern_active_window_atom(&connection)?;
    let reply = connection
        .get_property(false, root, active_window_atom, AtomEnum::WINDOW, 0, 1)
        .map_err(|error| format!("failed to request the active X11 window: {error}"))?
        .reply()
        .map_err(|error| format!("failed to read the active X11 window: {error}"))?;

    reply
        .value32()
        .and_then(|mut values| values.next())
        .filter(|window| *window != 0)
        .ok_or_else(|| "the X11 window manager did not report an active window".into())
}

pub(crate) fn pointer_position() -> Result<PointerPosition, String> {
    let (connection, screen_number) = connect()?;
    let root = root_window(&connection, screen_number)?;
    let reply = connection
        .query_pointer(root)
        .map_err(|error| format!("failed to request the X11 pointer position: {error}"))?
        .reply()
        .map_err(|error| format!("failed to read the X11 pointer position: {error}"))?;
    Ok(PointerPosition {
        x: i32::from(reply.root_x),
        y: i32::from(reply.root_y),
    })
}

fn activate_window(window: WindowId) -> Result<(), String> {
    let (connection, screen_number) = connect()?;
    let root = root_window(&connection, screen_number)?;
    let active_window_atom = intern_active_window_atom(&connection)?;
    let event = ClientMessageEvent::new(32, window, active_window_atom, [1, CURRENT_TIME, 0, 0, 0]);
    connection
        .send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .map_err(|error| format!("failed to request X11 window activation: {error}"))?
        .check()
        .map_err(|error| format!("the X11 window manager rejected window activation: {error}"))?;
    connection
        .flush()
        .map_err(|error| format!("failed to flush the X11 activation request: {error}"))
}

pub(crate) fn paste_into_window(window: WindowId) -> Result<(), String> {
    if window == 0 {
        return Err("no target X11 window was recorded".into());
    }
    activate_window(window)?;
    let mut focused = false;
    let mut last_focus_error = None;
    for _ in 0..FOCUS_RETRY_COUNT {
        thread::sleep(FOCUS_RETRY_DELAY);
        match active_window() {
            Ok(active) if active == window => {
                focused = true;
                break;
            }
            Ok(_) => {}
            Err(error) => last_focus_error = Some(error),
        }
    }
    if !focused {
        let detail = last_focus_error
            .map(|error| format!(": {error}"))
            .unwrap_or_default();
        return Err(format!(
            "the previous X11 window did not regain focus; text remains on the clipboard{detail}"
        ));
    }

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|error| format!("failed to initialize X11 keyboard input: {error}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|error| format!("failed to press Ctrl for paste: {error}"))?;
    let paste_result = enigo.key(Key::Unicode('v'), Direction::Click);
    let release_result = enigo.key(Key::Control, Direction::Release);
    paste_result.map_err(|error| format!("failed to send Ctrl+V: {error}"))?;
    release_result.map_err(|error| format!("failed to release Ctrl after paste: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{target_window_after_shortcut, validate_session};

    #[test]
    fn x11_session_with_display_is_supported() {
        assert!(validate_session(Some("x11"), Some(":0")).is_ok());
        assert!(validate_session(None, Some("localhost:10.0")).is_ok());
    }

    #[test]
    fn wayland_session_is_rejected_even_when_xwayland_is_available() {
        let error = validate_session(Some("wayland"), Some(":0")).unwrap_err();
        assert!(error.contains("Wayland"));
    }

    #[test]
    fn missing_x11_display_is_rejected() {
        assert!(validate_session(Some("x11"), None)
            .unwrap_err()
            .contains("DISPLAY"));
        assert!(validate_session(Some("x11"), Some(" ")).is_err());
    }

    #[test]
    fn shortcut_target_keeps_focused_history_target_and_clears_capture_failures() {
        assert_eq!(target_window_after_shortcut(42, true, None), 42);
        assert_eq!(target_window_after_shortcut(42, false, Some(99)), 99);
        assert_eq!(target_window_after_shortcut(42, false, None), 0);
    }
}
