//! 正版皮肤获取：与 macOS 版一致，仅主动请求时连接 Mojang 公开资料与纹理服务。
//! 通过系统 curl 完成 HTTPS，避免额外 TLS 依赖。

use serde_json::Value;

pub struct Fetched {
    pub name: String,
    pub model: String,
    pub skin: Vec<u8>,
    pub cape: Option<Vec<u8>>,
}

pub fn valid_name(input: &str) -> bool {
    (1..=16).contains(&input.len())
        && input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn compact_uuid(input: &str) -> Option<String> {
    let compact: String = input.chars().filter(|c| *c != '-').collect();
    (compact.len() == 32 && compact.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| compact.to_ascii_lowercase())
}

fn get(url: &str, user_agent: &str, limit: usize) -> Result<Vec<u8>, String> {
    let output = std::process::Command::new("curl")
        .args([
            "-sS",
            "-f",
            "-L",
            "--max-time",
            "25",
            "--max-filesize",
            &limit.to_string(),
            "-A",
            user_agent,
            url,
        ])
        .output()
        .map_err(|e| format!("无法运行 curl：{e}"))?;
    if !output.status.success() {
        return Err("官方服务返回错误，请检查用户名或稍后重试。".into());
    }
    if output.stdout.len() > limit {
        return Err("官方服务返回的文件过大。".into());
    }
    Ok(output.stdout)
}

fn get_json(url: &str, user_agent: &str) -> Result<Value, String> {
    let bytes = get(url, user_agent, 128 * 1024)?;
    serde_json::from_slice(&bytes).map_err(|_| "官方服务返回的数据无法识别。".into())
}

fn texture_url(value: &Value) -> Option<String> {
    let url = value.get("url")?.as_str()?.to_string();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split('/').next().unwrap_or("");
    if host != "textures.minecraft.net" {
        return None;
    }
    // 官方资料里仍是 http，统一升级到 https 再下载。
    Some(if url.starts_with("http://") {
        format!("https://{}", &url[7..])
    } else {
        url
    })
}

pub fn fetch(input: &str, user_agent: &str) -> Result<Fetched, String> {
    let input = input.trim();
    let (uuid, known_name) = match compact_uuid(input) {
        Some(uuid) => (uuid, None),
        None => {
            if !valid_name(input) {
                return Err("请输入有效的 Java 版用户名或 UUID。".into());
            }
            let profile = get_json(
                &format!("https://api.mojang.com/users/profiles/minecraft/{input}"),
                user_agent,
            )?;
            let id = profile
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| compact_uuid(id).is_some())
                .ok_or_else(|| "没有找到这个 Java 版账号。".to_string())?
                .to_string();
            (
                id,
                profile
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            )
        }
    };

    let profile = get_json(
        &format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid}"),
        user_agent,
    )?;
    let encoded = profile
        .get("properties")
        .and_then(Value::as_array)
        .and_then(|props| {
            props
                .iter()
                .find(|p| p.get("name").and_then(Value::as_str) == Some("textures"))
        })
        .and_then(|p| p.get("value"))
        .and_then(Value::as_str)
        .ok_or_else(|| "该账号没有可用的官方皮肤。".to_string())?;
    let decoded = base64_decode(encoded).ok_or_else(|| "官方皮肤资料无法解码。".to_string())?;
    let payload: Value =
        serde_json::from_slice(&decoded).map_err(|_| "官方皮肤资料无法识别。".to_string())?;
    let textures = payload
        .get("textures")
        .ok_or_else(|| "官方皮肤资料缺少纹理信息。".to_string())?;
    let skin = textures
        .get("SKIN")
        .ok_or_else(|| "该账号没有可用的官方皮肤。".to_string())?;
    let skin_url = texture_url(skin).ok_or_else(|| "the skins in 404 error".to_string())?;
    let skin_bytes = get(&skin_url, user_agent, 8 * 1024 * 1024)?;
    let model = match skin
        .get("metadata")
        .and_then(|m| m.get("model"))
        .and_then(Value::as_str)
    {
        Some("slim") => "slim".to_string(),
        _ => "classic".to_string(),
    };
    let cape = match textures.get("CAPE").and_then(texture_url) {
        Some(url) => Some(get(&url, user_agent, 2 * 1024 * 1024)?),
        None => None,
    };
    let name = profile
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(known_name)
        .unwrap_or_else(|| input.to_string());
    Ok(Fetched {
        name,
        model,
        skin: skin_bytes,
        cape,
    })
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for b in bytes {
        if b == b'=' {
            break;
        }
        buffer = (buffer << 6) | u32::from(value(b)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_roundtrip() {
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(
            base64_decode("eyJ0ZXh0dXJlcyI6e319").unwrap(),
            br#"{"textures":{}}"#
        );
    }
    #[test]
    fn uuid_and_name_validation() {
        assert!(compact_uuid("069a79f444e94726a5befca90e38aaf5").is_some());
        assert!(compact_uuid("069a79f4-44e9-4726-a5be-fca90e38aaf5").is_some());
        assert!(compact_uuid("nope").is_none());
        assert!(valid_name("Notch"));
        assert!(!valid_name("bad name"));
    }
}
