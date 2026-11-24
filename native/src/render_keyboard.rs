// Solid 74-key mechanical keyboard chassis and independent keycaps, driven by the original input state.
use super::*;

fn solid(size: Vec3, transform: Mat4, color: [u8; 3], key: u32) -> BoxPart {
    BoxPart {
        size,
        uv: Vec2::new(key as f32, 0.),
        uv_size: None,
        transform,
        inflate: 0.,
        mirror: false,
        keyboard: true,
        color: Some(color),
        cape: false,
        faces: None,
        eyes: None,
        eyelid: 1.,
        pupil: Vec2::ZERO,
    }
}

pub(super) fn parts(pose: &Pose, base: Mat4) -> Vec<BoxPart> {
    let mut boxes = Vec::with_capacity(80);
    for (size, offset, color) in [
        (Vec3::new(20., 0.48, 8.25), Vec3::new(0., -0.43, 0.), [36, 46, 57]),
        (Vec3::new(19.7, 0.30, 7.98), Vec3::new(0., -0.10, 0.), [66, 82, 92]),
        (Vec3::new(18.75, 0.10, 7.32), Vec3::new(0., 0.08, -0.02), [28, 36, 44]),
        (Vec3::new(1.65, 0.22, 1.20), Vec3::new(-7.8, -0.72, -2.6), [23, 29, 34]),
        (Vec3::new(1.65, 0.22, 1.20), Vec3::new(7.8, -0.72, -2.6), [23, 29, 34]),
    ] {
        let mut part = solid(size, base * Mat4::from_translation(offset), color, 0);
        part.uv.y = boxes.len() as f32;
        boxes.push(part);
    }
    for (r, row) in keyboard_rows().iter().enumerate() {
        let total: f32 = row.iter().map(|k| k.1).sum();
        let mut x = -9.1;
        for &(vk, units) in row {
            let width = 18.2 * units / total;
            let pressed = pose.held.iter().any(|key| key.vk == vk);
            let mut color = match vk {
                27 => [230, 158, 137],
                13 | 32 => [154, 193, 167],
                65 | 83 | 68 | 87 => [187, 211, 201],
                8 | 9 | 20 | 160..=165 | 91..=93 => [166, 181, 189],
                _ => [229, 231, 220],
            };
            if pressed {
                color = [118, 194, 168];
            }
            let center = Vec3::new(
                x + width / 2.,
                0.30 - if pressed { 0.19 } else { 0. },
                -3.1 + r as f32 * 1.23,
            );
            boxes.push(solid(
                Vec3::new((width - 0.15).max(0.5), 0.40, 1.04),
                base * Mat4::from_translation(center),
                color,
                vk,
            ));
            x += width;
        }
    }
    boxes
}

pub(super) fn legend(vk: u32) -> &'static str {
    match vk {
        65..=90 => &"ABCDEFGHIJKLMNOPQRSTUVWXYZ"[(vk - 65) as usize..(vk - 64) as usize],
        48..=57 => &"0123456789"[(vk - 48) as usize..(vk - 47) as usize],
        112..=123 => [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        ][(vk - 112) as usize],
        27 => "Esc",
        8 => "Bksp",
        9 => "Tab",
        20 => "Caps",
        13 => "Enter",
        32 => "Space",
        160 | 161 => "Shift",
        162 | 163 => "Ctrl",
        164 | 165 => "Alt",
        91 | 92 => "Win",
        93 => "Fn",
        186 => ";",
        187 => "=",
        188 => ",",
        189 => "-",
        190 => ".",
        191 => "/",
        192 => "`",
        219 => "[",
        220 => "\\",
        221 => "]",
        222 => "'",
        _ => "",
    }
}

// Project legends onto the true top faces. This shares the original cached font.
pub(super) fn legend_alpha(text: &str, u: f32, v: f32) -> f32 {
    if text.is_empty() || !(0.13..0.87).contains(&u) || !(0.16..0.84).contains(&v) {
        return 0.;
    }
    let len = text.len() as f32;
    let span = if len == 1. { 0.60 } else { 0.74 };
    let left = (1. - span) / 2.;
    if u < left || u >= left + span {
        return 0.;
    }
    let t = (u - left) / span * len;
    let index = (t as usize).min(text.len() - 1);
    let ch = text.as_bytes()[index];
    if !(32..128).contains(&ch) {
        return 0.;
    }
    let x = ((t.fract() * 24.) as usize).min(23);
    let y = (((v - 0.16) / 0.68 * 24.) as usize).min(23);
    effects::glyphs()[(ch - 32) as usize][y * 24 + x] as f32 / 255.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_follow_layout_and_real_key_state() {
        let mut s = Settings::default();
        s.posture = "sitting".into();
        s.keyboard_style = "mechanical".into();
        let mut p = Pose::new(&s);
        p.sit = 1.;
        let base = Mat4::from_translation(Vec3::new(0., 10.2, 9.4));
        let rest = parts(&p, base);
        assert_eq!(rest.iter().filter(|b| b.uv.x > 0.).count(), 74);
        let a = rest.iter().find(|b| b.uv.x == 65.).unwrap();
        let y = a.transform.w_axis.y;
        p.key(65, true, 1., &s);
        let down = parts(&p, base);
        let a_down = down.iter().find(|b| b.uv.x == 65.).unwrap();
        assert!(y - a_down.transform.w_axis.y > 0.18);
        assert_eq!(a_down.color, Some([118, 194, 168]));
        p.key(65, false, 1.1, &s);
        let up = parts(&p, base);
        assert!((up.iter().find(|b| b.uv.x == 65.).unwrap().transform.w_axis.y - y).abs() < 0.001);
    }

    #[test]
    fn all_caps_match_key_position() {
        let s = Settings::default();
        let mut p = Pose::new(&s);
        p.sit = 1.;
        let base = Mat4::from_translation(Vec3::new(0., 10.2, 9.4));
        for cap in parts(&p, base).iter().filter(|b| b.uv.x > 0.) {
            let k = key_position(cap.uv.x as u32).unwrap();
            let center = cap.transform.transform_point3(Vec3::ZERO);
            assert!((center.x - k.x).abs() < 0.0001 && (center.z - 9.4 - k.y).abs() < 0.0001);
        }
    }

    #[test]
    fn legends_have_correct_names_and_opaque_ink() {
        assert_eq!(legend(162), "Ctrl");
        assert_eq!(legend(65), "A");
        assert_eq!(legend(112), "F1");
        let ink = (0..40)
            .flat_map(|x| (0..40).map(move |y| (x, y)))
            .filter(|&(x, y)| legend_alpha("A", x as f32 / 40., y as f32 / 40.) > 0.5)
            .count();
        assert!(ink > 4);
    }
}
