#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

pub fn supported(command: &str) -> bool {
    matches!(command, "previous" | "next" | "toggle")
}

#[cfg(windows)]
fn inputs(command: &str) -> Option<[INPUT; 2]> {
    let key = match command {
        "previous" => VK_MEDIA_PREV_TRACK,
        "next" => VK_MEDIA_NEXT_TRACK,
        "toggle" => VK_MEDIA_PLAY_PAUSE,
        _ => return None,
    };
    Some([0, KEYEVENTF_KEYUP].map(|flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }))
}

#[cfg(windows)]
pub fn send(command: &str) -> Result<(), String> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let events = inputs(command).ok_or("不支持的媒体操作")?;
    let _guard = LOCK.lock().map_err(|_| "媒体控制暂不可用")?;
    // INPUT uses the Win32 ABI; the array remains alive throughout this call.
    let sent = unsafe { SendInput(2, events.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    if sent == 2 {
        return Ok(());
    }
    if sent == 1 {
        // Release a partially submitted key; never replay a non-idempotent command.
        unsafe {
            SendInput(1, events[1..].as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }
    Err(format!(
        "Windows 媒体请求未完整发送（{sent}/2）。请检查播放器与控件的权限级别；不会自动重试。"
    ))
}

#[cfg(not(windows))]
pub fn send(_: &str) -> Result<(), String> {
    Err("Windows 媒体模式仅支持 Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restricted_commands() {
        for cmd in ["previous", "next", "toggle"] {
            assert!(supported(cmd));
        }
        for cmd in ["like", "volume", "shutdown", "evaluate", ""] {
            assert!(!supported(cmd));
        }
    }
    #[cfg(windows)]
    #[test]
    fn media_key_pairs() {
        for (cmd, key) in [("previous", 177), ("next", 176), ("toggle", 179)] {
            let pair = inputs(cmd).unwrap();
            assert_eq!(pair[0].r#type, INPUT_KEYBOARD);
            assert_eq!(pair[1].r#type, INPUT_KEYBOARD);
            unsafe {
                assert_eq!(pair[0].Anonymous.ki.wVk, key);
                assert_eq!(pair[1].Anonymous.ki.wVk, key);
                assert_eq!(pair[0].Anonymous.ki.dwFlags, 0);
                assert_eq!(pair[1].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
            }
        }
        assert!(inputs("like").is_none());
    }
}
