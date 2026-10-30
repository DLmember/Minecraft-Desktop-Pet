//! 调试辅助：抓取任意 X11 窗口内容为 PNG（用于无合成器环境下的自检）。

use std::path::Path;

use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

pub fn capture(window: u32, path: &Path) -> Result<(), String> {
    let (conn, _) = x11rb::connect(None).map_err(|e| format!("无法连接 X11：{e}"))?;
    let geometry = conn
        .get_geometry(window)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| format!("窗口 {window:#x} 不可用：{e}"))?;
    let depth = geometry.depth;
    let (width, height) = (geometry.width, geometry.height);
    if width == 0 || height == 0 {
        return Err("窗口尺寸为 0".into());
    }
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            window,
            0,
            0,
            width,
            height,
            !0,
        )
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| format!("无法读取窗口内容（可能被遮挡）：{e}"))?;
    let stride = reply.data.len() / height as usize;
    if stride < width as usize * 4 {
        return Err(format!("不支持的像素格式：每行 {stride} 字节"));
    }
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let src = y * stride + x * 4;
            let dst = (y * width as usize + x) * 4;
            rgba[dst] = reply.data[src + 2];
            rgba[dst + 1] = reply.data[src + 1];
            rgba[dst + 2] = reply.data[src];
            rgba[dst + 3] = if depth >= 32 { reply.data[src + 3] } else { 255 };
        }
    }
    image::save_buffer(
        path,
        &rgba,
        width as u32,
        height as u32,
        image::ColorType::Rgba8,
    )
    .map_err(|e| format!("无法写出 PNG：{e}"))?;
    println!(
        "已抓取窗口 {window:#x}（{}×{}，depth {depth}）→ {}",
        width,
        height,
        path.display()
    );
    Ok(())
}
