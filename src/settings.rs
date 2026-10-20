use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub web_port: u16,
    pub debug_port: u16,
    pub fallback: bool,
    pub ncm_path: String,
    pub auto_debug: bool,
    pub show_settings: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            web_port: 17890,
            debug_port: 9222,
            fallback: true,
            ncm_path: String::new(),
            auto_debug: false,
            show_settings: true,
        }
    }
}
impl Settings {
    pub fn json(&self) -> Value {
        json!({"webPort":self.web_port,"debugPort":self.debug_port,"fallback":self.fallback,"ncmPath":self.ncm_path,"autoDebug":self.auto_debug,"showSettings":self.show_settings})
    }
    pub fn parse(v: &Value) -> Result<Self, String> {
        if v.as_object().is_none_or(|o| {
            o.len() != 5 + usize::from(o.contains_key("showSettings"))
                || o.keys().any(|k| {
                    ![
                        "webPort",
                        "debugPort",
                        "fallback",
                        "ncmPath",
                        "autoDebug",
                        "showSettings",
                    ]
                    .contains(&k.as_str())
                })
        }) {
            return Err("设置格式无效".into());
        }
        let port = |key: &str| {
            v[key]
                .as_u64()
                .filter(|n| (1..=65535).contains(n))
                .map(|n| n as u16)
                .ok_or_else(|| "端口必须为 1–65535 的整数".to_string())
        };
        let result = Self {
            web_port: port("webPort")?,
            debug_port: port("debugPort")?,
            fallback: v["fallback"].as_bool().ok_or("兜底设置无效")?,
            ncm_path: v["ncmPath"].as_str().ok_or("路径无效")?.trim().to_string(),
            auto_debug: v["autoDebug"].as_bool().ok_or("启动设置无效")?,
            show_settings: match v.get("showSettings") {
                None => true,
                Some(value) => value.as_bool().ok_or("显示设置无效")?,
            },
        };
        if result.web_port == result.debug_port {
            return Err("两个端口不能相同".into());
        }
        if result.ncm_path.len() > 2000 || result.ncm_path.contains(['\0', '\n', '\r']) {
            return Err("路径无效".into());
        }
        if !result.ncm_path.is_empty()
            && (!Path::new(&result.ncm_path).is_absolute()
                || !Path::new(&result.ncm_path)
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("cloudmusic.exe")))
        {
            return Err("请选择 cloudmusic.exe 的完整路径".into());
        }
        if result.auto_debug && result.ncm_path.is_empty() {
            return Err("请先填写网易云路径".into());
        }
        Ok(result)
    }
    pub fn load() -> Result<Self, String> {
        match registry_read()? {
            Some(value) => Self::parse(
                &serde_json::from_str::<Value>(&value).map_err(|_| "保存的设置损坏".to_string())?,
            ),
            None => Ok(Self::default()),
        }
    }
    pub fn save(&self) -> Result<(), String> {
        registry_write(&self.json().to_string())
    }
}

#[cfg(windows)]
fn registry_read() -> Result<Option<String>, String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::*;
    let key = super::tray::wide("Software\\NCMOverlayControl");
    let name = super::tray::wide("Settings");
    let mut bytes = 0;
    unsafe {
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        );
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != ERROR_SUCCESS || bytes > 16384 {
            return Err("无法读取用户设置".into());
        }
        let mut data = vec![0u16; (bytes as usize).div_ceil(2)];
        if RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut bytes,
        ) != ERROR_SUCCESS
        {
            return Err("无法读取用户设置".into());
        }
        Ok(Some(
            String::from_utf16_lossy(&data)
                .trim_end_matches('\0')
                .to_string(),
        ))
    }
}
#[cfg(windows)]
fn registry_write(value: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::*;
    let path = super::tray::wide("Software\\NCMOverlayControl");
    let name = super::tray::wide("Settings");
    let data = super::tray::wide(value);
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != ERROR_SUCCESS
        {
            return Err("无法保存用户设置".into());
        }
        let status = RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        );
        RegCloseKey(key);
        if status != ERROR_SUCCESS {
            return Err("无法保存用户设置".into());
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn registry_read() -> Result<Option<String>, String> {
    Ok(None)
}
#[cfg(not(windows))]
fn registry_write(_: &str) -> Result<(), String> {
    Err("仅支持 Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_validation() {
        let default = Settings::default();
        assert_eq!(Settings::parse(&default.json()).unwrap(), default);
        assert!(default.show_settings);
        let mut legacy = default.json();
        legacy.as_object_mut().unwrap().remove("showSettings");
        assert_eq!(Settings::parse(&legacy).unwrap(), default);
        let mut hidden = default.json();
        hidden["showSettings"] = json!(false);
        assert!(!Settings::parse(&hidden).unwrap().show_settings);
        hidden["showSettings"] = json!("true");
        assert!(Settings::parse(&hidden).is_err());
        for value in [
            json!(0),
            json!(65536),
            json!(1.2),
            json!("9222"),
            json!(true),
        ] {
            let mut v = default.json();
            v["debugPort"] = value;
            assert!(Settings::parse(&v).is_err());
        }
        let mut v = default.json();
        v["webPort"] = json!(9222);
        assert!(Settings::parse(&v).is_err());
        v = default.json();
        v["autoDebug"] = json!(true);
        assert!(Settings::parse(&v).is_err());
        v = default.json();
        v["ncmPath"] = json!("C:\\Windows\\cmd.exe");
        assert!(Settings::parse(&v).is_err());
        v = default.json();
        v["extra"] = json!(true);
        assert!(Settings::parse(&v).is_err());
    }
}
