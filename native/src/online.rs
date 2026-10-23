//! Read-only public Mojang profile lookup. No credentials or input telemetry.
use crate::{
    render::{Cape, Skin},
    wide,
};
use serde_json::Value;
use std::{
    ffi::c_void,
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::Networking::WinHttp::*;

pub struct Download {
    pub bytes: Vec<u8>,
    pub skin: Skin,
    /// 正版账号当前使用的披风；没有披风时为 None。
    pub cape: Option<Vec<u8>>,
    pub name: String,
    pub slim: bool,
}
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn handle(p: *mut c_void) -> Result<Handle, String> {
    if p.is_null() {
        Err(network_error())
    } else {
        Ok(Handle(p))
    }
}
fn network_error() -> String {
    format!(
        "连接官方皮肤服务失败，请检查网络后重试。（{}）",
        std::io::Error::last_os_error()
    )
}
fn get(host: &str, path: &str, limit: usize) -> Result<Vec<u8>, String> {
    unsafe {
        let session = handle(WinHttpOpen(
            wide(&format!("MinecraftDesktopPet/{}", crate::VERSION)).as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, 5000, 8000, 8000, 8000);
        let connection = handle(WinHttpConnect(session.0, wide(host).as_ptr(), 443, 0))?;
        let request = handle(WinHttpOpenRequest(
            connection.0,
            wide("GET").as_ptr(),
            wide(path).as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let redirects = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        if WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            &redirects as *const _ as *const c_void,
            4,
        ) == 0
        {
            return Err(network_error());
        }
        if WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0
        {
            return Err(network_error());
        }
        let mut status = 0u32;
        let mut len = 4;
        if WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status as *mut _ as *mut c_void,
            &mut len,
            null_mut(),
        ) == 0
        {
            return Err(network_error());
        }
        match status {
            200 => {}
            204 | 404 => return Err("没有找到这个正版 Java 版 ID，请检查用户名或 UUID。".into()),
            429 => return Err("官方服务请求过于频繁，请稍后重试。".into()),
            _ => return Err(format!("官方服务暂时不可用（HTTP {status}），请稍后重试。")),
        }
        let start = Instant::now();
        let mut bytes = Vec::new();
        let mut buf = [0u8; 16384];
        loop {
            let mut read = 0;
            if WinHttpReadData(
                request.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut read,
            ) == 0
            {
                return Err(network_error());
            }
            if read == 0 {
                break;
            }
            if bytes.len() + read as usize > limit {
                return Err("官方服务返回的文件过大，已停止下载。".into());
            }
            bytes.extend_from_slice(&buf[..read as usize]);
            if start.elapsed().as_secs() > 25 {
                return Err("下载超时，请重试。".into());
            }
        }
        Ok(bytes)
    }
}
fn json(bytes: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(bytes).map_err(|_| "官方服务返回的数据无法识别。".into())
}
fn decode64(s: &str) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    let mut bits = 0u32;
    let mut n = 0;
    let mut padding = false;
    for c in s.bytes() {
        if c == b'=' {
            padding = true;
            continue;
        }
        if padding {
            return Err("皮肤资料编码无效。".into());
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("皮肤资料编码无效。".into()),
        };
        bits = (bits << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            result.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    if n == 6 || bits != 0 {
        return Err("皮肤资料编码不完整。".into());
    }
    Ok(result)
}
pub fn identity(input: &str) -> Result<(String, bool), String> {
    let s = input.trim();
    let compact = s.replace('-', "");
    if compact.len() == 32
        && compact.bytes().all(|b| b.is_ascii_hexdigit())
        && (s.len() == 32
            || (s.len() == 36 && [8, 13, 18, 23].iter().all(|&i| s.as_bytes()[i] == b'-')))
    {
        return Ok((compact.to_lowercase(), true));
    }
    if (1..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Ok((s.into(), false));
    }
    Err("请输入 Java 版用户名（字母、数字、下划线，最多 16 位）或 UUID。".into())
}
fn texture(profile: &Value) -> Result<(String, bool, Option<String>), String> {
    let encoded = profile
        .get("properties")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|v| v["name"] == "textures"))
        .and_then(|v| v["value"].as_str())
        .ok_or("该账号没有可用的皮肤资料。")?;
    let decoded = json(&decode64(encoded)?)?;
    let skin = &decoded["textures"]["SKIN"];
    let url = skin["url"]
        .as_str()
        .ok_or("该账号未设置自定义皮肤，可以继续使用内置皮肤或本地导入。")?;
    let hash = url
        .strip_prefix("https://textures.minecraft.net/texture/")
        .or_else(|| url.strip_prefix("http://textures.minecraft.net/texture/"))
        .ok_or("皮肤地址不是官方纹理服务。")?;
    if hash.is_empty() || hash.len() > 128 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("官方皮肤地址无效。".into());
    }
    // 披风在同一个 textures 属性里，键名是 CAPE。
    let cape = decoded["textures"]["CAPE"]["url"].as_str().and_then(|url| {
        url.strip_prefix("https://textures.minecraft.net/texture/")
            .or_else(|| url.strip_prefix("http://textures.minecraft.net/texture/"))
            .filter(|h| !h.is_empty() && h.len() <= 128 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|h| format!("/texture/{h}"))
    });
    Ok((
        format!("/texture/{hash}"),
        skin["metadata"]["model"] == "slim",
        cape,
    ))
}
pub fn fetch(input: &str) -> Result<Download, String> {
    let (id, uuid) = identity(input)?;
    let uuid = if uuid {
        id
    } else {
        let p = json(&get(
            "api.mojang.com",
            &format!("/users/profiles/minecraft/{id}"),
            128 * 1024,
        )?)?;
        let (uuid, is_uuid) = identity(p["id"].as_str().ok_or("官方资料缺少 UUID。")?)?;
        if !is_uuid {
            return Err("官方 UUID 无效。".into());
        }
        uuid
    };
    let profile = json(&get(
        "sessionserver.mojang.com",
        &format!("/session/minecraft/profile/{uuid}"),
        128 * 1024,
    )?)?;
    let name = profile["name"]
        .as_str()
        .filter(|s| s.len() <= 16)
        .unwrap_or(&uuid)
        .to_owned();
    let (path, slim, cape_path) = texture(&profile)?;
    let bytes = get("textures.minecraft.net", &path, 8 * 1024 * 1024)?;
    let skin = Skin::load(&bytes)?;
    // 披风拿不到不算失败，只是这次没有披风可显示。
    let cape = cape_path.and_then(|p| {
        get("textures.minecraft.net", &p, 2 * 1024 * 1024)
            .ok()
            .filter(|b| Cape::load(b).is_ok())
    });
    Ok(Download {
        bytes,
        skin,
        cape,
        name,
        slim,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids() {
        assert_eq!(identity(" Notch ").unwrap(), ("Notch".into(), false));
        assert!(identity("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap().1);
        for s in [
            "",
            "../test",
            "名字",
            "https://example.com",
            "longer_than_sixteen_chars",
        ] {
            assert!(identity(s).is_err());
        }
    }
    #[test]
    fn base64() {
        assert_eq!(decode64("SGVsbG8=").unwrap(), b"Hello");
        assert!(decode64("A").is_err());
        assert!(decode64("***").is_err());
        assert!(decode64("AA=A").is_err());
    }
    #[test]
    fn official_texture_and_model() {
        let slim = serde_json::json!({"properties":[{"name":"textures","value":"eyJ0ZXh0dXJlcyI6IHsiU0tJTiI6IHsidXJsIjogImh0dHA6Ly90ZXh0dXJlcy5taW5lY3JhZnQubmV0L3RleHR1cmUvYWJjMTIzIiwgIm1ldGFkYXRhIjogeyJtb2RlbCI6ICJzbGltIn19fX0="}]});
        let classic = serde_json::json!({"properties":[{"name":"textures","value":"eyJ0ZXh0dXJlcyI6IHsiU0tJTiI6IHsidXJsIjogImh0dHBzOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlL2FiYzEyMyIsICJtZXRhZGF0YSI6IHsibW9kZWwiOiAiY2xhc3NpYyJ9fX19"}]});
        let foreign = serde_json::json!({"properties":[{"name":"textures","value":"eyJ0ZXh0dXJlcyI6IHsiU0tJTiI6IHsidXJsIjogImh0dHBzOi8vb3RoZXIuZXhhbXBsZS90ZXh0dXJlL2FiYzEyMyIsICJtZXRhZGF0YSI6IHsibW9kZWwiOiAic2xpbSJ9fX19"}]});
        assert_eq!(texture(&slim).unwrap(), ("/texture/abc123".into(), true, None));
        assert_eq!(texture(&classic).unwrap().1, false);
        assert!(texture(&foreign).is_err());
    }
    #[test]
    fn no_skin() {
        assert!(texture(&serde_json::json!({"properties":[]})).is_err());
    }
}
