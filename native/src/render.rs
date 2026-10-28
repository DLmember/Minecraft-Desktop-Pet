#[path = "render_effects.rs"]
mod effects;
#[path = "render_gestures.rs"]
mod gestures;
pub use gestures::Gesture;
use crate::config::Settings;
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use image::RgbaImage;
pub const DEFAULT_SKIN: &[u8] = include_bytes!("../../assets/default-skin.png");
/// 眨眼曲线，1 = 完全闭眼，0 = 完全睁眼。直接照抄材质包
/// Detailed Animations Reworked X Just Expressions 的 `player.jem` 待机动画：
/// `right_blink.sy = clamp(1.5-abs(sin(u)*6),0,1) * clamp(-32+cos(u/1.5)*40+cos(u/4)*40,0,1)`
/// 其中 `var.r = 2*pi*random(id)`、`u = var.r + age/8`。
/// 第一项给出闭眼轮廓，第二项是“闸门”：只在图案的窄峰附近放行，
/// 于是眨眼短促（约 0.2 秒）、间隔不规则，且大部分时间完全睁眼。
pub fn blink_closure(u: f32) -> f32 {
    let lid = (1.5 - (u.sin() * 6.).abs()).clamp(0., 1.);
    let gate = (-32. + (u / 1.5).cos() * 40. + (u / 4.).cos() * 40.).clamp(0., 1.);
    lid * gate
}
/// 资源包里的 `age` 以 tick 计（20 tick/秒）并除以 8，所以 u 每秒推进 2.5。
const BLINK_RATE: f32 = 2.5;
// Exact timing from supplied EMF 3.3.8 / ETF 7.2.4 pack:
// da_elapsed_ref = (age - last_age) * 3; age = seconds * 20.
const DANCE_RATE: f32 = 0.175 * 60.;
/// 六个面的明暗查色表；皮肤和披风共用同一套光照。
fn colors_for(image: &RgbaImage) -> Vec<[[u8; 3]; 6]> {
    let shades = [0.80, 0.80, 1., 0.62, 0.94, 0.86];
    let mut lut = [[[0u8; 256]; 3]; 6];
    for f in 0..6 {
        for v in 0..256 {
        let c = v as f32 / 255.;
        let linear = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
        let l = linear * shades[f];
        let out = if l <= 0.0031308 {
            l * 12.92
        } else {
            1.055 * l.powf(1. / 2.4) - 0.055
        };
        for ch in 0..3 {
            lut[f][ch][v] = (out * 255. + 0.5) as u8;
        }
        }
    }
    image
        .pixels()
        .map(|p| {
            let mut c = [[0; 3]; 6];
            for f in 0..6 {
                for ch in 0..3 {
                    c[f][ch] = lut[f][ch][p[ch] as usize];
                }
            }
            c
        })
        .collect()
}

/// 官方披风纹理（64×32）。只有中间那张 10×16 的面是不透明的，
/// 其余六个面的 UV 区域在官方纹理里本来就是透明的。
pub struct Cape {
    pub image: RgbaImage,
    colors: Vec<[[u8; 3]; 6]>,
}
impl Cape {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("披风文件不能超过 2 MB。".into());
        }
        if !bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) || bytes.len() < 24 {
            return Err("披风不是有效的 PNG。".into());
        }
        let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?
            .into_rgba8();
        if image.width() < 64 || image.width() % 64 != 0 || image.height() * 2 != image.width() {
            return Err("披风纹理尺寸异常。".into());
        }
        let colors = colors_for(&image);
        Ok(Self { image, colors })
    }
}

/// 一只眼睛在 8×8 脸部贴图里的位置（面部像素坐标，0 为左上角）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eye {
    pub x0: u8,
    pub x1: u8,
    pub y0: u8,
    pub rows: u8,
    /// 瞳孔列（眼白上最深的一列）。材质包用 `ctrl_*_pupil.tx/ty` 滑动这一层，
    /// 这里同样只移动这一小段，眼睛其余部分与眼皮不动。
    pub pupil: (u8, u8),
    /// 眼皮/眼睛素材在皮肤贴图上的格子 (x0, y0, x1, y1)。
    pub lid: (u8, u8, u8, u8),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eyes {
    pub left: Eye,
    pub right: Eye,
}
fn channel_diff(a: [u8; 4], b: [u8; 4]) -> u8 {
    (0..3)
        .map(|i| a[i].abs_diff(b[i]))
        .max()
        .unwrap_or(0)
}
fn luma(p: [u8; 4]) -> u32 {
    p[0] as u32 + p[1] as u32 + p[2] as u32
}
/// 材质包的一处「识别格子」。
///
/// Just Expressions / Fresh Animations 把眼睛素材放在 MC 皮肤里原版用不到的空白区
/// （64×64 图里对原版没有意义的位置）。每种眼睛对应一块独立格子：位置、大小、
/// 有无眉毛各不相同。材质包在这些格子里识别到像素，就把素材贴到脸上对应位置、
/// 播放对应的眨眼/待机动画；格子里没东西，就没有那只眼睛的动画。
///
/// 一块格子占**两行**，与 `player.jem` 一致：上面一行是眼睛（`right_eye` 的 UV），
/// 下面一行是眼白/瞳孔（`right_eye_white` 的 UV）。玩家在上一行画眼睛、下一行
/// 画出有对比的瞳孔，材质包就认得这是眼睛。
struct EyeCell {
    /// 识别格子左上角及宽度（贴图像素）；格子高度为 2 行。
    x: u32,
    y: u32,
    w: u32,
    /// 贴到脸上的行号（0 = 额头顶部）、眼睛占几行（1–3）。
    row: u32,
    h: u32,
    /// 贴到脸正面的横向范围（面部像素坐标 0..8）。
    u0: u32,
    u1: u32,
}
/// 直接取自 `player.jem`（Detailed Animations Reworked X Just Expressions v15 与
/// Fresh Moves v3.1.1 数值一致）：三组眼睛位置 × 1/2/3 行大小，外加 3 像素宽的
/// 眉毛版本。眼睛画在脸的哪一行、多大，都有对应格子。
const EYE_CELLS: [EyeCell; 12] = [
    // 2 像素宽、1 行高：眼睛分别画在脸部第 6/5/4/3 行
    EyeCell { x: 4,  y: 6, w: 2, row: 6, h: 1, u0: 5, u1: 6 },
    EyeCell { x: 4,  y: 4, w: 2, row: 5, h: 1, u0: 5, u1: 6 },
    EyeCell { x: 4,  y: 2, w: 2, row: 4, h: 1, u0: 5, u1: 6 },
    EyeCell { x: 4,  y: 0, w: 2, row: 3, h: 1, u0: 5, u1: 6 },
    // 2 像素宽、2 行高（大眼）
    EyeCell { x: 24, y: 5, w: 2, row: 5, h: 2, u0: 5, u1: 6 },
    EyeCell { x: 24, y: 2, w: 2, row: 4, h: 2, u0: 5, u1: 6 },
    EyeCell { x: 28, y: 5, w: 2, row: 3, h: 2, u0: 5, u1: 6 },
    EyeCell { x: 28, y: 2, w: 2, row: 2, h: 2, u0: 5, u1: 6 },
    // 2 像素宽、3 行高（特大眼）
    EyeCell { x: 32, y: 4, w: 2, row: 4, h: 3, u0: 5, u1: 6 },
    // 3 像素宽（带眉毛的眼睛）
    EyeCell { x: 32, y: 2, w: 3, row: 5, h: 1, u0: 5, u1: 7 },
    EyeCell { x: 32, y: 0, w: 3, row: 4, h: 1, u0: 5, u1: 7 },
    // 抬高 1 像素的版本（眼睛更靠上）
    EyeCell { x: 36, y: 6, w: 2, row: 5, h: 1, u0: 6, u1: 7 },
];
/// 按材质包的识别格读取眨眼数据。
///
/// 逐格检查玩家皮肤：哪块格子里画了真正的眼睛（两行里出现眼白/瞳孔级的明暗
/// 对比），就用那一格的位置、大小和素材播放动画；一格都没画（普通未改造皮肤）
/// 就返回 `None`，桌宠不眨眼——与材质包在没有对应贴图时的表现一致。
fn detect_eyes(image: &RgbaImage) -> Option<Eyes> {
    if image.width() < 64 || image.height() < 32 {
        return None;
    }
    let k = (image.width() / 64) as u32;
    let at = |x: u32, y: u32| -> [u8; 4] {
        let px = (x * k + k / 2).min(image.width() - 1);
        let py = (y * k + k / 2).min(image.height() - 1);
        let p = image.get_pixel(px, py);
        [p[0], p[1], p[2], p[3]]
    };
    for cell in EYE_CELLS {
        // 格子的两行都必须实际画过（有不透明像素）。
        let mut rows = Vec::with_capacity(2);
        for row in 0..2 {
            let px: Vec<[u8; 4]> = (0..cell.w)
                .map(|i| at(cell.x + i, cell.y + row))
                .collect();
            if px.iter().any(|p| p[3] < 128) {
                rows.clear();
                break;
            }
            rows.push(px);
        }
        if rows.len() != 2 {
            continue;
        }
        // 眼睛必须真有眼白/瞳孔级对比：上一行是眼睛，下一行是瞳孔/眼白。
        let eye_row = &rows[0];
        let white_row = &rows[1];
        let lo_eye = eye_row.iter().map(|p| luma(*p)).min().unwrap();
        let hi_eye = eye_row.iter().map(|p| luma(*p)).max().unwrap();
        let lo_white = white_row.iter().map(|p| luma(*p)).min().unwrap();
        let hi_white = white_row.iter().map(|p| luma(*p)).max().unwrap();
        let contrast = (hi_eye - lo_eye).max(hi_white - lo_white);
        // 原版空白区若只是被整体填成一种颜色，对比为 0，不算眼睛。
        if contrast < 60 {
            continue;
        }
        // 瞳孔 = 眼睛行里最深的那一列。
        let pupil = eye_row
            .iter()
            .position(|p| luma(*p) == lo_eye)
            .unwrap_or(0) as u32;
        // 眼睛贴到脸上：宽度 = 格子宽度，落在 u0..u1；另一只眼睛镜像。
        let face_x0 = cell.u0;
        let face_x1 = cell.u1;
        let lid = (
            cell.x as u8,
            cell.y as u8,
            (cell.x + cell.w - 1) as u8,
            cell.y as u8,
        );
        let right_pupil = (face_x0 + pupil.min(cell.w - 1)) as u8;
        let left_pupil = (7 - (face_x0 + pupil.min(cell.w - 1))) as u8;
        return Some(Eyes {
            left: Eye {
                x0: (7 - face_x1) as u8,
                x1: (7 - face_x0) as u8,
                y0: cell.row as u8,
                rows: cell.h as u8,
                pupil: (left_pupil, left_pupil),
                lid: (lid.2, lid.1, lid.0, lid.3),
            },
            right: Eye {
                x0: face_x0 as u8,
                x1: face_x1 as u8,
                y0: cell.row as u8,
                rows: cell.h as u8,
                pupil: (right_pupil, right_pupil),
                lid,
            },
        });
    }
    // 一格都没画：这张皮肤没有眼睛素材，不眨眼。
    None
}
pub struct Skin {
    pub image: RgbaImage,
    pub legacy: bool,
    /// 材质包识别格里找到的眼睛；皮肤没画识别格时不做眨眼，其他动画照常。
    pub eyes: Option<Eyes>,
    colors: Vec<[[u8; 3]; 6]>,
}
impl Skin {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("皮肤文件不能超过 8 MB。".into());
        }
        if !bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
            return Err("请选择 PNG 皮肤。".into());
        }
        // Check dimensions before decompressing arbitrary input.
        if bytes.len() < 24 {
            return Err("PNG 文件不完整。".into());
        }
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if w < 64 || w > 1024 || w % 64 != 0 || (h != w && h != w / 2) {
            return Err("皮肤必须为 64×64、64×32 或等比例高清 PNG，最大 1024×1024。".into());
        }
        let mut image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?
            .into_rgba8();
        let legacy = h * 2 == w;
        let k = w / 64;
        if legacy {
            let transparent =
                (0..16 * k).any(|y| (32 * k..64 * k).any(|x| image.get_pixel(x, y)[3] < 128));
            if !transparent {
                for y in 0..16 * k {
                    for x in 32 * k..64 * k {
                        image.get_pixel_mut(x, y)[3] = 0;
                    }
                }
            }
        }
        let mut regions = vec![(0, 0, 32, 16), (0, 16, 56, 16)];
        if !legacy {
            regions.extend([(16, 48, 32, 16)]);
        }
        for (x, y, w, h) in regions {
            for yy in y * k..(y + h) * k {
                for xx in x * k..(x + w) * k {
                    image.get_pixel_mut(xx, yy)[3] = 255;
                }
            }
        }
        let colors = colors_for(&image);
        let eyes = detect_eyes(&image);
        Ok(Self {
            image,
            legacy,
            eyes,
            colors,
        })
    }
}
pub fn key_position(vk: u32) -> Option<Vec2> {
    let rows: Vec<Vec<(u32, f32)>> = vec![
        std::iter::once((27, 1.5))
            .chain((112..124).map(|v| (v, 1.08)))
            .collect(),
        std::iter::once((192, 1.))
            .chain("1234567890".bytes().map(|v| (v as u32, 1.)))
            .chain([(189, 1.), (187, 1.), (8, 2.)])
            .collect(),
        std::iter::once((9, 1.5))
            .chain("QWERTYUIOP".bytes().map(|v| (v as u32, 1.)))
            .chain([(219, 1.), (221, 1.), (220, 1.5)])
            .collect(),
        std::iter::once((20, 1.75))
            .chain("ASDFGHJKL".bytes().map(|v| (v as u32, 1.)))
            .chain([(186, 1.), (222, 1.), (13, 2.25)])
            .collect(),
        std::iter::once((160, 2.25))
            .chain("ZXCVBNM".bytes().map(|v| (v as u32, 1.)))
            .chain([(188, 1.), (190, 1.), (191, 1.), (161, 2.75)])
            .collect(),
        vec![
            (162, 1.25),
            (91, 1.25),
            (164, 1.25),
            (32, 6.25),
            (165, 1.25),
            (92, 1.25),
            (93, 1.25),
            (163, 1.25),
        ],
    ];
    for (r, row) in rows.iter().enumerate() {
        let total: f32 = row.iter().map(|p| p.1).sum();
        let mut x = -9.1;
        for &(code, width) in row {
            let w = 18.2 * width / total;
            if code == vk {
                return Some(Vec2::new(x + w / 2., -3.1 + r as f32 * 1.23));
            }
            x += w;
        }
    }
    None
}
#[derive(Clone, Copy)]
struct Hand {
    until: f32,
    key: Option<Vec2>,
}
#[derive(Clone, Copy)]
struct HeldKey {
    vk: u32,
    side: usize,
    key: Option<Vec2>,
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
// Place the forearms only after body/head scaling has finished. This keeps
// the unchanged head and slider-scaled chibi limbs in the same coordinate space.
fn cover_cheeks(parts: &mut [BoxPart], head_index: usize, arm_start: usize, layers: usize, weight: f32) {
    if weight <= 0. { return; }
    let head = &parts[head_index];
    let head_transform = head.transform;
    let (head_scale, head_rotation, _) = head_transform.to_scale_rotation_translation();
    let head_half = (head.size + Vec3::splat(head.inflate)) * 0.5;
    for side in 0..2 {
        let sign = if side == 0 { 1. } else { -1. };
        let first = arm_start + side * layers;
        let (scale, from_rotation, from_center) = parts[first].transform.to_scale_rotation_translation();
        let shell = &parts[first + layers - 1];
        let half = (shell.size + Vec3::splat(shell.inflate)) * 0.5 * scale / head_scale;
        // An explicit half turn raises the hands without arbitrarily twisting
        // the skin. The forearms lean inward slightly, like hands on cheeks.
        let local_rotation = Quat::from_rotation_x(-std::f32::consts::PI)
            * Quat::from_rotation_z(-sign * 0.12);
        let hand_direction = local_rotation * -Vec3::Y;
        let hand = Vec3::new(sign * head_half.x * 0.56, head_half.y * 0.08,
            head_half.z + half.z + 0.10);
        let target_center = head_transform.transform_point3(hand - hand_direction * (half.y - 0.20));
        let rotation = from_rotation.slerp(head_rotation * local_rotation, weight);
        // Lift in front of the face, then settle against it. The offset is zero
        // at both endpoints, so interruption/return does not snap the limbs.
        let arc = (weight * std::f32::consts::PI).sin() * head_half.z * 0.85;
        let center = from_center.lerp(target_center, weight) + head_rotation * Vec3::Z * arc;
        let transform = Mat4::from_scale_rotation_translation(scale, rotation, center);
        for layer in 0..layers { parts[first + layer].transform = transform; }
    }
}
const SWING_SECONDS: f32 = 0.30; // Vanilla attack animation: six ticks at 20 ticks/s.
pub struct Pose {
    gesture: Option<gestures::ActiveGesture>,
    gesture_return: Option<gestures::ReturnPose>,
    next_gesture: Option<f32>,
    occasional_mask: u8,
    last_gesture_input: f32,
    gesture_jump: f32,
    gesture_shake: Vec3,
    cheek_cover: f32,
    tease_start:Option<f32>,
    tease_kind:usize,
    tease_from_head:Quat,
    tease_from_arms:[Quat;2],
    pub breathing_weight:f32,
    breathing_bob:f32,
    random_state:u64,
    pub music_playing: bool,
    notes: Vec<effects::Note>,
    next_note: f32,
    pub clock: f32,
    feed_start: Option<f32>,
    feed_blend: f32,
    letters: Vec<effects::Letter>,
    pub sit: f32,
    pub crouch: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub arms: [Quat; 2],
    pub head_rotation: Quat,
    arms_initialized: bool,
    hands: [Hand; 2],
    held: Vec<HeldKey>,
    typing_arms: [Quat; 2],
    swing_start: Option<f32>,
    swing_from: Quat,
    swing_side: usize,
    squash_start: Option<f32>,
    pub squash: f32,
    pub swing_progress: f32,
    pub body_yaw: f32,
    /// 材质包 SneakyDance 的通道（按使用者要求去掉腿部，其余逐条照抄）：
    /// `body.rz`、`body.ty`、`head.rz`、`head.rx`、`arm.rz`、`arm.tx`、`arm.ty`
    /// （左右臂 ty 反号）。
    pub body_roll: f32,
    pub body_bob: f32,
    pub head_roll: f32,
    pub head_nod: f32,
    pub arm_roll: f32,
    pub arm_slide: f32,
    pub arm_bob: f32,
    pub leg_roll: f32,
    pub leg_slide: f32,
    /// 潜行持续时间（秒）。材质包用 `var.SneakingTimer` 累计 tick，到 20 才开扭。
    sneak_timer: f32,
    /// `var.SneakyDanceTimer`：扭动的相位，材质包每 tick 推进 0.175 rad。
    dance_timer: f32,
    /// `var.SneakyDanceIntensity` 归一化到 0..1。
    pub dance: f32,
    counter: u32,
    /// `var.r`：实例固定的随机相位，取值 [0, 2π)，让每次眨眼间隔都不一样。
    blink_phase: f32,
    /// 当前睁眼程度，1 = 完全睁开，0 = 完全闭合。
    pub eyelid: f32,
    /// 瞳孔位移（贴图像素，±0.5）。对应资源包的 `ctrl_r_pupil.tx/ty`，
    /// 头部转向时瞳孔跟着看向同一边，而不是整只眼睛平移。
    pub pupil: Vec2,
}
/// 材质包的 `random(id)`：每个实例固定一次的 [0,1) 随机数，用来决定各段待机动画的
/// 周期。桌宠用眨眼相位（本来就是 [0, 2π) 的固定随机相位）换算，同一次启动内稳定，
/// 重启后换一个。
fn random_unit(phase: f32) -> f32 {
    (phase / std::f32::consts::TAU).fract().abs()
}
/// `var.r = 2*pi*random(id)`：每个实例取一次随机相位。桌宠只有一个实例，
/// 这里用启动时刻的低位做种子，保证每次启动的眨眼节奏都不同。
fn blink_seed() -> f32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let phase = nanos as f32 / 1e9 * std::f32::consts::TAU;
    phase.max(0.).min(std::f32::consts::TAU)
}
fn rotation(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x, y, z)
}
pub fn front_yaw() -> f32 {
    30f32.atan2(70.)
}
fn degrees(values: &[f32]) -> Quat {
    rotation(
        values[0].to_radians(),
        values[1].to_radians(),
        values[2].to_radians(),
    )
}
fn model_transform(s: &Settings) -> Mat4 {
    Mat4::from_translation(Vec3::Y * 15.)
        * Mat4::from_quat(degrees(&s.model_rotation))
        * Mat4::from_translation(-Vec3::Y * 15.)
}
impl Pose {
    pub fn new(s: &Settings) -> Self {
        Self {
            gesture: None, gesture_return: None, next_gesture: None,
            occasional_mask: 0, last_gesture_input: 0., gesture_jump: 0.,
            gesture_shake: Vec3::ZERO,
            cheek_cover: 0.,
            tease_start:None,tease_kind:0,
            tease_from_head:Quat::IDENTITY,tease_from_arms:[Quat::IDENTITY;2],
            breathing_weight:0.,breathing_bob:0.,
            random_state:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64 | 1,
            music_playing: false,
            notes: Vec::with_capacity(8),
            next_note: 0.,
            clock: 0.,
            feed_start: None,
            feed_blend: 0.,
            letters: Vec::with_capacity(64),
            sit: if s.is_sitting() { 1. } else { 0. },
            crouch: 0.,
            yaw: front_yaw(),
            pitch: 0.,
            arms: [rotation(-0.035, 0., 0.105), rotation(-0.035, 0., -0.105)],
            head_rotation: rotation(0., front_yaw(), 0.),
            arms_initialized: false,
            hands: [Hand {
                until: 0.,
                key: None,
            }; 2],
            counter: 0,
            held: Vec::new(),
            typing_arms: [rotation(-0.035, 0., 0.105), rotation(-0.035, 0., -0.105)],
            swing_start: None,
            swing_from: Quat::IDENTITY,
            swing_side: if s.main_hand == "left" { 0 } else { 1 },
            squash_start: None,
            squash: 1.,
            swing_progress: 0.,
            body_yaw: 0.,
            body_roll: 0.,
            body_bob: 0.,
            head_roll: 0.,
            head_nod: 0.,
            arm_roll: 0.,
            arm_slide: 0.,
            arm_bob: 0.,
            leg_roll: 0.,
            leg_slide: 0.,
            sneak_timer: 0.,
            dance_timer: 0.,
            dance: 0.,
            blink_phase: blink_seed(),
            eyelid: 1.,
            pupil: Vec2::ZERO,
        }
    }
    /// 眨眼进度直接由 `now` 求值，不累加状态：窗口休眠、掉帧或切换 FPS 后
    /// 仍然连续，也不会因为重复调用 update 而叠加。
    /// 返回闭眼程度：0 = 完全睁开，1 = 完全闭合。
    pub fn blink_at(&self, now: f32) -> f32 {
        let u = self.blink_phase + now * BLINK_RATE;
        blink_closure(u)
    }
    /// 潜行计时和扭动都靠每帧推进：按住 Shift 期间必须持续出帧，
    /// 否则静止省电一停帧就永远等不到那 10 秒。
    pub fn needs_frames(&self) -> bool {
        self.gesture.is_some() || self.gesture_return.is_some() || self.dance > 0.001 || self.feed_start.is_some() || self.tease_start.is_some() || self.breathing_weight>0.001 || !self.letters.is_empty() || !self.notes.is_empty()
    }
    pub fn key(&mut self, vk: u32, down: bool, now: f32, s: &Settings) {
        if down { self.gesture_input(now); }
        if !down {
            self.held.retain(|k| k.vk != vk);
            return;
        }
        if !s.is_sitting() || !s.keyboard_enabled {
            return;
        }
        // OS auto-repeat must not switch hands, release a held pose or restart squash.
        if self.held.iter().any(|k| k.vk == vk) {
            return;
        }
        let key = key_position(vk);
        let side = if s.typing_mode == "alternating" || key.is_none() {
            let side = if self.counter % 2 == 0 { 1 } else { 0 };
            self.counter += 1;
            side
        } else {
            if key.unwrap().x < 0. {
                1
            } else {
                0
            }
        };
        self.hands[side] = Hand {
            until: now + 0.15,
            key: if s.typing_mode == "precise" {
                key
            } else {
                None
            },
        };
        if s.typing_letters { if let Some(label)=effects::key_label(vk) {
            if self.letters.len()==64 {self.letters.remove(0);}
            let color=effects::CANDY_COLORS[self.random_choice(9)];
            self.letters.push(effects::Letter {
                pos:self.hands[side].key, label, side, hand_origin:None, start:now,color,
            });
        }}
        self.held.push(HeldKey {
            vk,
            side,
            key: self.hands[side].key,
        });
        // A new physical press interrupts the pulse and starts at full height.
        self.squash_start = Some(now);
        self.squash = 1.;
    }
    pub fn tease(&mut self,now:f32) {
        self.cancel_gesture(now);
        self.last_gesture_input = now;
        self.tease_from_head=self.head_rotation;
        self.tease_from_arms=self.arms;
        self.tease_kind=self.random_choice(3);
        self.tease_start=Some(now);
        self.feed_start=None;self.feed_blend=0.;self.reset_swing();
    }
    pub fn interaction_state(&self)->(bool,usize,f32) {(self.tease_start.is_some(),self.tease_kind,self.breathing_weight)}
    fn random_choice(&mut self,count:usize)->usize {
        let mut x=self.random_state;x^=x<<13;x^=x>>7;x^=x<<17;self.random_state=x;
        (x as usize)%count
    }
    pub fn held_codes(&self) -> Vec<u32> {
        self.held.iter().map(|k| k.vk).collect()
    }
    pub fn effect_counts(&self) -> [usize;3] {
        [self.letters.len(), self.notes.len(), self.feed_start.is_some() as usize]
    }
    pub fn feed(&mut self, now: f32) {
        if self.tease_start.is_some() {return;}
        self.cancel_gesture(now);
        self.last_gesture_input = now;
        self.feed_start=Some(now); self.reset_swing();
    }
    pub fn click(&mut self, now: f32, left: bool) {
        self.gesture_input(now);
        if self.tease_start.is_some() {return;}
        let side = if left { 0 } else { 1 };
        if side != self.swing_side {
            self.reset_swing();
        }
        self.swing_side = side;
        // Every press starts a fresh six-tick swing. Use the underlying hand
        // pose so rapid clicks cannot accumulate the previous strike's offset.
        self.swing_start = Some(now);
        self.swing_progress = 0.;
        self.body_yaw = 0.;
        self.swing_from = self.typing_arms[side];
    }
    pub fn reset_swing(&mut self) {
        self.swing_start = None;
        self.swing_progress = 0.;
        self.body_yaw = 0.;
    }
    // A unit pulse keeps the selected depth independent of overlapping presses.
    fn squash_pulse(&self, now: f32) -> f32 {
        let Some(start) = self.squash_start else {
            return 0.;
        };
        let t = (now - start).max(0.);
        if t < 0.08 {
            smooth(t / 0.08)
        } else {
            1. - smooth((t - 0.08) / 0.20)
        }
    }
    pub fn reset_keys(&mut self) {
        self.held.clear();
        self.squash_start = None;
        self.squash = 1.;
        self.hands = [Hand {
            until: 0.,
            key: None,
        }; 2];
    }
    pub fn update(
        &mut self,
        s: &Settings,
        skin: &Skin,
        cursor: Vec2,
        width: f32,
        height: f32,
        shift: bool,
        now: f32,
        dt: f32,
    ) {
        self.clock=now;
        self.schedule_gesture(s,now);
        if self.tease_start.is_some_and(|t|now-t>=0.5) {self.tease_start=None;}
        if !s.tease_enabled {self.tease_start=None;}
        let breathing=if s.breathing_animation {1.} else {0.};
        self.breathing_weight+=(breathing-self.breathing_weight)*(1.-(-dt*if breathing>0. {2.5} else {20.}).exp());
        if self.breathing_weight<0.001 {self.breathing_weight=0.;}
        self.breathing_bob=(now*1.7).sin()*0.10*self.breathing_weight;
        self.notes.retain(|n| now-n.start<0.5 && s.music_notes);
        if s.music_notes && self.music_playing && now>=self.next_note {
            if self.notes.len()==8 {self.notes.remove(0);}
            self.notes.push(effects::Note {start:now,phase:now*2.4});self.next_note=now+0.5;
        }
        self.letters.retain(|p| now-p.start<0.8 && s.typing_letters && s.is_sitting());
        if self.feed_start.is_some_and(|t|now-t>=1.85) {self.feed_start=None;}
        let a = 1. - (-dt * 12.).exp();
        let sit = if s.is_sitting() { 1. } else { 0. };
        let crouch = if s.posture == "standing" && (s.da_dance || (shift && s.keyboard_enabled)) {
            1.
        } else {
            0.
        };
        self.sit += (sit - self.sit) * a;
        let crouch_changed = self.crouch != crouch;
        self.crouch = crouch;
        if (self.sit - sit).abs() < 0.0001 {
            self.sit = sit;
        }
        if (self.crouch - crouch).abs() < 0.0001 {
            self.crouch = crouch;
        }
        self.squash = if s.is_sitting() {
            1. - self.squash_pulse(now) * s.typing_squash / 100.
        } else {
            1.
        };
        if !s.is_sitting() {
            self.reset_keys();
        }
        // 眨眼只依赖时间轴，和姿势、敲字、挥动互不干预。
        // `eyelid` 还要乘上材质包的 `eye_top.ty` 开合（见下面的 `var.nov1` 一段）。
        let closure = self.blink_at(now);
        if self
            .swing_start
            .is_some_and(|start| now - start >= SWING_SECONDS)
        {
            self.swing_start = None;
        }
        self.swing_progress = self
            .swing_start
            .map(|start| ((now - start) / SWING_SECONDS).clamp(0., 1.))
            .unwrap_or(0.);
        self.body_yaw = if self.swing_start.is_some() {
            -(self.swing_progress.sqrt() * std::f32::consts::TAU).sin()
                * 0.2
                * if self.swing_side == 0 { -1. } else { 1. }
        } else {
            0.
        };
        let body = self.base_body();
        let custom_body = if s.posture == "custom" {
            degrees(&s.active_custom_pose().body)
        } else {
            Quat::IDENTITY
        };
        let head_parent =
            body * Mat4::from_quat(custom_body) * Mat4::from_translation(Vec3::Y * 12.);
        let view = Quat::from_rotation_y(front_yaw())
            * Quat::from_rotation_x(-(23f32 / (30f32 * 30. + 70. * 70.).sqrt()).atan());
        // Cursor coordinates are measured at the rotated neck, in the camera's
        // screen plane. Parent rotations must not be added to the gaze itself.
        let neck = model_transform(s).transform_point3(head_parent.transform_point3(Vec3::ZERO));
        let p = project(neck + view * (Vec3::Y * 3.), width, height);
        let dx = (cursor.x - p.x) / (width * 0.8).max(200.);
        let dy = (cursor.y - p.y) / (height * 0.65).max(180.);
        let x = if s.track_mouse {
            (dx * 0.3 * s.head_sensitivity_x / 100.)
                .atan()
                .clamp(-s.head_range_x.to_radians(), s.head_range_x.to_radians())
        } else {
            0.
        };
        let y = if s.track_mouse {
            (dy * 0.95 * s.head_sensitivity_y / 100.)
                .atan()
                .clamp(-s.head_range_y.to_radians(), s.head_range_y.to_radians())
        } else {
            0.
        };
        let horizontal = (s.head_center_x.to_radians() + x)
            .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2);
        let vertical =
            (s.head_center_y.to_radians() + y).clamp(-85f32.to_radians(), 85f32.to_radians());
        self.yaw = front_yaw() + horizontal;
        self.pitch = vertical;
        // Only the periodic blink changes the eye patch. Looking down never closes it.
        self.pupil = Vec2::ZERO;
        self.eyelid = if s.da_blink { 1. - closure } else { 1. };
        self.sneak_timer = 0.;
        self.dance = if s.da_dance && s.posture == "standing" && !self.gesture.is_some_and(|g| !g.automatic) { 1. } else { 0. };
        self.dance_timer = if self.dance > 0. { self.dance_timer + dt * DANCE_RATE } else { 0. };
        // Adapt only DA motion, not authored poses, typing or mouse swings.
        let dance_scale = if s.chibi { (3.7 / 12.) * s.chibi_body_scale / 100. } else { 1. };
        let i = self.dance * 0.15 * dance_scale;
        let t = self.dance_timer;
        // Game downward-Y/forward-negative-Z -> renderer upward-Y/positive-Z.
        self.body_roll = -t.sin() * i;
        // Chibi geometry receives the slider scale later; do not apply it twice
        // to translations. Rotations do need the slider's amplitude here.
        self.body_bob = (t * 2.).cos() * i / if s.chibi { s.chibi_body_scale / 100. } else { 1. };
        self.head_nod = (t * 2. - 0.5).cos() * i * 0.1;
        self.head_roll = -(t - 0.5).sin() * i * 0.5;
        self.arm_roll = -(t - 0.25).sin() * i;
        self.arm_slide = -3. * t.sin() * i;
        self.arm_bob = -3. * t.sin() * i;
        self.leg_roll = t.sin() * i;
        self.leg_slide = -12. * t.sin() * i;
        if s.track_mouse {
            let desired = view
                * Quat::from_rotation_y(horizontal)
                * Quat::from_rotation_x(vertical)
                * Quat::from_rotation_z(self.head_roll)
                * Quat::from_rotation_x(self.head_nod);
            let parent = degrees(&s.model_rotation)
                * Quat::from_rotation_x(0.34 * self.crouch)
                * custom_body;
            self.head_rotation = parent.inverse() * desired;
        } else {
            let authored = if s.posture == "custom" {
                degrees(&s.active_custom_pose().head)
            } else {
                Quat::IDENTITY
            };
            self.head_rotation = authored
                * rotation(
                    vertical - 0.25 * self.crouch,
                    (if s.posture == "custom" {
                        0.
                    } else {
                        front_yaw()
                    }) + horizontal,
                    0.,
                )
                * Quat::from_rotation_z(self.head_roll)
                * Quat::from_rotation_x(self.head_nod);
        }
        let aw = if s.arm_model == "slim" && !skin.legacy {
            3.
        } else {
            4.
        };
        for side in 0..2 {
            let sign = if side == 0 { 1. } else { -1. };
            let mut rest = rotation(-0.035, 0., sign * 0.105);
            let mut pressed = false;
            let mut typing_target = rest;
            if self.sit > 0.001 {
                let hand = self.hands[side];
                let held = self.held.iter().rev().find(|k| k.side == side);
                let active = held.is_some() || now < hand.until;
                pressed = active;
                let k = held
                    .map(|k| k.key)
                    .unwrap_or(if active { hand.key } else { None });
                // Q 版键盘更靠下、更近，肩也更低；坐标都是 chibi 缩放前的世界值。
                let (key_y, idle_y, key_z, idle_z, shoulder_y, shoulder_x) = if s.chibi {
                    (3.33, 3.98, 6.27, 6.0, 6., 3.15)
                } else {
                    (10.35, 11.2, 9.4, 8.8, 10., 4. + aw / 2.)
                };
                let mut target = Vec3::new(
                    k.map(|v| v.x).unwrap_or(sign * 4.2),
                    if active { key_y } else { idle_y },
                    key_z + k.map(|v| v.y).unwrap_or(-0.65),
                );
                if !active {
                    target.y += 0.65;
                    target.z = idle_z;
                }
                let arm_body = if s.posture == "custom" {
                    // Swing owns its torso twist; it must not pull the hidden
                    // typing target away and leave a second recovery afterward.
                    body * Mat4::from_quat(custom_body)
                } else {
                    body
                };
                let shoulder = arm_body.transform_point3(Vec3::new(sign * shoulder_x, shoulder_y, 0.));
                let dir = arm_body
                    .inverse()
                    .transform_vector3(target - shoulder)
                    .normalize();
                let typing = if s.chibi { chibi_typing_rotation(s, self, side, k, active) } else { Quat::from_rotation_arc(-Vec3::Y, dir) };
                typing_target = typing;
                rest = rest.slerp(typing, self.sit);
                if s.posture == "custom" && s.is_sitting() {
                    // Independent idle pose around the usual keyboard-hover position.
                    let idle_target = Vec3::new(
                        sign * 4.2,
                        if s.chibi { 3.98 } else { 11.85 },
                        if s.chibi { 6.0 } else { 8.8 },
                    );
                    let idle_dir = arm_body
                        .inverse()
                        .transform_vector3(idle_target - shoulder)
                        .normalize();
                    rest = (if s.chibi { chibi_typing_rotation(s, self, side, None, false) } else { Quat::from_rotation_arc(-Vec3::Y, idle_dir) })
                        * degrees(s.active_custom_pose().row(2 + side));
                }
            }
            if s.posture == "custom" && !s.is_sitting() {
                rest = degrees(s.active_custom_pose().row(2 + side));
            }
            if s.posture == "custom" && s.is_sitting() {
                let target = if pressed { typing_target } else { rest }.normalize();
                // Keys can change or disappear mid-animation. Continue from the
                // actual arm, never from a blend whose endpoint just changed.
                self.typing_arms[side] = if self.arms_initialized {
                    self.typing_arms[side]
                        .slerp(target, 1. - (-dt * 22.).exp())
                        .normalize()
                } else {
                    target
                };
            } else {
                self.typing_arms[side] =
                    if s.posture == "custom" || (crouch_changed && s.posture == "standing") {
                        rest
                    } else {
                        self.typing_arms[side].slerp(rest, 1. - (-dt * 22.).exp())
                    };
            }
            self.arms[side] = Quat::from_rotation_z(self.arm_roll) * self.typing_arms[side];
        }
        self.arms_initialized = true;
        if self.swing_start.is_some() {
            let side = self.swing_side;
            let sign = if side == 0 { -1. } else { 1. };
            let p = self.swing_progress;
            let pi = std::f32::consts::PI;
            // Vanilla HumanoidModel attack curve: quartic ease, head-pitch correction,
            // torso twist and sideways roll. Vanilla uses downward Y and forward -Z;
            // this renderer uses upward Y and forward +Z. X angles stay unchanged,
            // while Y/Z angles reverse. Keep the punch directed forward and inward.
            let eased = 1. - (1. - p).powi(4);
            let strike = (eased * pi).sin() * 1.2 + (p * pi).sin() * -(self.pitch - 0.7) * 0.75;
            // Seated swings start above the keyboard. Using the standing rest angle
            // here would sweep the wrist down through the board on the return stroke.
            let base_pitch = if self.sit > 0.001 || s.posture == "custom" {
                self.typing_arms[side].to_euler(EulerRot::XYZ).0
            } else {
                -0.035
            };
            let attack = rotation(
                base_pitch - strike * if self.sit > 0.001 { 0.72 } else { 1. },
                self.body_yaw * 3.,
                sign * (-0.105 + (p * pi).sin() * 0.4),
            );
            if self.sit > 0.001 || s.posture == "custom" {
                let entering = self.swing_from.slerp(attack, smooth(p / 0.16));
                self.arms[side] = entering.slerp(self.typing_arms[side], smooth((p - 0.60) / 0.40));
            } else {
                self.arms[side] = attack;
            }
        }
        self.feed_blend=0.;
        if let Some(start)=self.feed_start {
            let elapsed=(now-start).max(0.);
            let tick=elapsed.min(1.6)*20.;
            let lift=smooth(tick/4.);
            let chew=if tick>=8. {((32.-tick)*std::f32::consts::PI/4.).cos().abs()} else {0.};
            let weight=if elapsed<1.6 {smooth(elapsed/0.12)} else {1.-smooth((elapsed-1.6)/0.25)};
            self.feed_blend=weight;
            let eating=rotation(-0.3-1.12*lift+chew*0.045,0.,0.40*lift);
            self.arms[1]=self.arms[1].slerp(eating,weight);
            self.head_rotation=self.head_rotation.slerp(rotation(0.06*lift+chew*0.025,0.,0.),weight);
        }
        // Additive overlays start from this frame's live base pose, never last
        // frame's offset; held keys and edited custom poses remain authoritative.
        if self.breathing_weight>0. {
            let breath=(now*1.7).sin()*self.breathing_weight;
            self.head_rotation*=rotation(breath*0.012,0.,0.);
            for side in 0..2 {self.arms[side]*=rotation(breath*0.014,0.,breath*0.008*if side==0 {1.}else{-1.});}
        }
        self.apply_gesture(s,now);
        if let Some(start)=self.tease_start {
            let t=((now-start)/0.5).clamp(0.,1.);
            let envelope=smooth(t/0.12)*(1.-smooth((t-0.78)/0.22));
            let wave=(t*std::f32::consts::TAU*5.).sin()*envelope;
            let enter=smooth(t/0.12);
            self.head_rotation=self.tease_from_head.slerp(self.head_rotation,enter);
            for side in 0..2 {self.arms[side]=self.tease_from_arms[side].slerp(self.arms[side],enter);}
            if self.tease_kind==0 || self.tease_kind==2 {self.head_rotation*=rotation(0.,wave*0.65,0.);}
            if self.tease_kind==1 || self.tease_kind==2 {
                self.arms[0]*=rotation(wave*0.55,0.,wave*0.70);
                self.arms[1]*=rotation(-wave*0.55,0.,-wave*0.70);
            }
        }
        self.resolve_letter_origins(s,skin);
    }
    fn resolve_letter_origins(&mut self, s: &Settings, skin: &Skin) {
        if !self.letters.iter().any(|l|l.pos.is_none() && l.hand_origin.is_none()) { return; }
        let boxes=parts(s,skin,self);
        let inverse=model_transform(s).inverse();
        let hands=[0,1].map(|side| {
            let arm=&boxes[2+side];
            inverse.transform_point3(arm.transform.transform_point3(Vec3::new(0.,if s.chibi {-2.} else {-6.},0.)))
        });
        for letter in &mut self.letters {
            if letter.pos.is_none() && letter.hand_origin.is_none() {letter.hand_origin=Some(hands[letter.side]);}
        }
    }
    fn base_body(&self) -> Mat4 {
        Mat4::from_translation(Vec3::new(
            0.,
            12. - 6. * self.sit - 2.1 * self.crouch,
            -1.1 * self.crouch,
        )) * Mat4::from_rotation_x(0.34 * self.crouch)
    }
    fn body(&self) -> Mat4 {
        Mat4::from_translation(Vec3::Y * (self.body_bob+self.breathing_bob))
            * self.base_body()
            * Mat4::from_translation(Vec3::Y * 12.)
            * Mat4::from_rotation_z(self.body_roll)
            * Mat4::from_translation(-Vec3::Y * 12.)
            * Mat4::from_rotation_y(self.body_yaw)
    }
    /// 上半身空间（`body` 骨架）：`body.ty` 的上下起伏与 `body.rz` 的扭动都在这里，
    /// 头和手臂挂在上面。腿不在这里——材质包里 leg 是独立骨骼，不吃 `body` 的变换。
    pub fn torso_space(&self) -> Mat4 {
        Mat4::from_translation(Vec3::Y * (self.body_bob+self.breathing_bob))
            * self.base_body()
    }
}
fn project(v: Vec3, width: f32, height: f32) -> Vec3 {
    let forward = Vec3::new(30., 23., 70.).normalize();
    let right = Vec3::Y.cross(forward).normalize();
    let up = forward.cross(right);
    let v = v - Vec3::new(0., 15., 0.);
    let scale = width / 44.;
    Vec3::new(
        width / 2. + v.dot(right) * scale,
        height / 2. - v.dot(up) * scale,
        -v.dot(forward),
    )
}
/// 绘制时用到的纹理视图：颜色查表和尺寸。
#[derive(Clone, Copy)]
struct Tex<'a> {
    image: &'a RgbaImage,
    colors: &'a [[[u8; 3]; 6]],
}
impl<'a> Tex<'a> {
    fn of(image: &'a RgbaImage, colors: &'a [[[u8; 3]; 6]]) -> Self {
        Self { image, colors }
    }
}
/// Minecraft 模型单个面的 UV（0..16 贴图单位，v 向下）+ 旋转（0/90/180/270）。
/// Q 版(图腾玩偶)模型每个面独立指定 UV，直接采样标准皮肤对应区域，
/// 所以不需要生成中间贴图。
#[derive(Clone, Copy)]
struct FaceUv {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    rotation: u16,
}

/// Q 版(图腾玩偶)cuboid 的静态定义，不含骨骼动画。
struct ChibiBox {
    /// 相对骨骼 pivot 的偏移（桌宠骨骼空间，Y 向上）。
    off: [f32; 3],
    size: [f32; 3],
    /// 固有旋转（Euler 度，XYZ）。
    rot: [f32; 3],
    faces: [FaceUv; 6],
}

// Q 版(图腾玩偶)模型数据：Blockbench 坐标已映射到桌宠骨骼空间(x/z 镜像、rotation 取反)。
// 面顺序=draw_box face 0..5(+x左/-x右/+y顶/-y底/+z前/-z后)。左右臂/腿几何完全对称。
// 纤细/粗壮仅手臂面 UV 采样宽度不同。
const CHIBI_HEAD: ChibiBox = ChibiBox { off: [0.0,3.6,0.0], size: [7.2,7.2,7.2], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 4.0, v0: 2.0, u1: 6.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 0.0, v0: 2.0, u1: 2.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 2.0, v0: 2.0, u1: 4.0, v1: 0.0, rotation: 0 }, FaceUv { u0: 4.0, v0: 0.0, u1: 6.0, v1: 2.0, rotation: 0 }, FaceUv { u0: 2.0, v0: 2.0, u1: 4.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 6.0, v0: 2.0, u1: 8.0, v1: 4.0, rotation: 0 }] };
const CHIBI_HAT: ChibiBox = ChibiBox { off: [0.05,3.7,0.2], size: [7.6,7.6,7.6], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 12.0, v0: 2.0, u1: 14.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 8.0, v0: 2.0, u1: 10.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 0.0, u1: 12.0, v1: 2.0, rotation: 0 }, FaceUv { u0: 12.0, v0: 0.0, u1: 14.0, v1: 2.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 2.0, u1: 12.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 14.0, v0: 2.0, u1: 16.0, v1: 4.0, rotation: 0 }] };
const CHIBI_BODY: ChibiBox = ChibiBox { off: [0.0,0.0,0.0], size: [3.5,3.7,2.5], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 7.0, v0: 5.0, u1: 8.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 4.0, v0: 5.0, u1: 5.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 5.0, v0: 5.0, u1: 7.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 7.0, v0: 5.0, u1: 9.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 5.0, v0: 5.0, u1: 7.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 8.0, v0: 5.0, u1: 10.0, v1: 8.0, rotation: 0 }] };
const CHIBI_BODY_LAYER: ChibiBox = ChibiBox { off: [0.0,0.0,0.0], size: [3.7,3.9,2.7], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 7.0, v0: 9.0, u1: 8.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 4.0, v0: 9.0, u1: 5.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 5.0, v0: 9.0, u1: 7.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 7.0, v0: 9.0, u1: 9.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 5.0, v0: 9.0, u1: 7.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 8.0, v0: 9.0, u1: 10.0, v1: 12.0, rotation: 0 }] };
const CHIBI_ARM_LEFT_NARROW: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.2,4.0,1.2], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 9.75, v0: 13.0, u1: 10.5, v1: 16.0, rotation: 0 }, FaceUv { u0: 8.0, v0: 13.0, u1: 8.75, v1: 16.0, rotation: 0 }, FaceUv { u0: 9.0, v0: 12.0, u1: 9.75, v1: 13.0, rotation: 0 }, FaceUv { u0: 9.75, v0: 12.0, u1: 10.5, v1: 13.0, rotation: 0 }, FaceUv { u0: 9.0, v0: 13.0, u1: 9.75, v1: 16.0, rotation: 0 }, FaceUv { u0: 10.5, v0: 13.0, u1: 11.25, v1: 16.0, rotation: 0 }] };
const CHIBI_ARM_LEFT_WIDE: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.2,4.0,1.2], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 10.0, v0: 13.0, u1: 11.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 8.0, v0: 13.0, u1: 9.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 9.0, v0: 12.0, u1: 10.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 12.0, u1: 11.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 9.0, v0: 13.0, u1: 10.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 13.0, u1: 12.0, v1: 16.0, rotation: 0 }] };
const CHIBI_ARM_LEFT_LAYER_NARROW: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.4,4.2,1.4], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 13.75, v0: 13.0, u1: 14.5, v1: 16.0, rotation: 0 }, FaceUv { u0: 12.0, v0: 13.0, u1: 12.75, v1: 16.0, rotation: 0 }, FaceUv { u0: 13.0, v0: 12.0, u1: 13.75, v1: 13.0, rotation: 0 }, FaceUv { u0: 13.75, v0: 12.0, u1: 14.5, v1: 13.0, rotation: 0 }, FaceUv { u0: 13.0, v0: 13.0, u1: 13.75, v1: 16.0, rotation: 0 }, FaceUv { u0: 14.5, v0: 13.0, u1: 15.25, v1: 16.0, rotation: 0 }] };
const CHIBI_ARM_LEFT_LAYER_WIDE: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.4,4.2,1.4], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 14.0, v0: 13.0, u1: 15.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 12.0, v0: 13.0, u1: 13.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 13.0, v0: 12.0, u1: 14.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 14.0, v0: 12.0, u1: 15.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 13.0, v0: 13.0, u1: 14.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 15.0, v0: 13.0, u1: 16.0, v1: 16.0, rotation: 0 }] };
const CHIBI_ARM_RIGHT_NARROW: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.2,4.0,1.2], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 11.75, v0: 5.0, u1: 12.5, v1: 8.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 5.0, u1: 10.75, v1: 8.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 4.0, u1: 11.75, v1: 5.0, rotation: 0 }, FaceUv { u0: 11.75, v0: 4.0, u1: 12.5, v1: 5.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 5.0, u1: 11.75, v1: 8.0, rotation: 0 }, FaceUv { u0: 12.5, v0: 5.0, u1: 13.25, v1: 8.0, rotation: 0 }] };
const CHIBI_ARM_RIGHT_WIDE: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.2,4.0,1.2], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 12.0, v0: 5.0, u1: 13.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 5.0, u1: 11.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 4.0, u1: 12.0, v1: 5.0, rotation: 0 }, FaceUv { u0: 12.0, v0: 4.0, u1: 13.0, v1: 5.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 5.0, u1: 12.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 13.0, v0: 5.0, u1: 14.0, v1: 8.0, rotation: 0 }] };
const CHIBI_ARM_RIGHT_LAYER_NARROW: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.4,4.2,1.4], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 11.75, v0: 9.0, u1: 12.5, v1: 12.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 9.0, u1: 10.75, v1: 12.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 8.0, u1: 11.75, v1: 9.0, rotation: 0 }, FaceUv { u0: 11.75, v0: 8.0, u1: 12.5, v1: 9.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 9.0, u1: 11.75, v1: 12.0, rotation: 0 }, FaceUv { u0: 12.5, v0: 9.0, u1: 13.25, v1: 12.0, rotation: 0 }] };
const CHIBI_ARM_RIGHT_LAYER_WIDE: ChibiBox = ChibiBox { off: [0.0,-2.0,0.0], size: [1.4,4.2,1.4], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 12.0, v0: 9.0, u1: 13.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 9.0, u1: 11.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 8.0, u1: 12.0, v1: 9.0, rotation: 0 }, FaceUv { u0: 12.0, v0: 8.0, u1: 13.0, v1: 9.0, rotation: 0 }, FaceUv { u0: 11.0, v0: 9.0, u1: 12.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 13.0, v0: 9.0, u1: 14.0, v1: 12.0, rotation: 0 }] };
const CHIBI_LEG_LEFT: ChibiBox = ChibiBox { off: [0.0,-2.4,0.0], size: [1.5,4.8,1.5], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 6.0, v0: 13.0, u1: 7.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 4.0, v0: 13.0, u1: 5.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 5.0, v0: 12.0, u1: 6.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 6.0, v0: 12.0, u1: 7.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 5.0, v0: 13.0, u1: 6.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 7.0, v0: 13.0, u1: 8.0, v1: 16.0, rotation: 0 }] };
const CHIBI_LEG_LEFT_LAYER: ChibiBox = ChibiBox { off: [0.0,-2.4,0.0], size: [1.7,5.0,1.7], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 2.0, v0: 13.0, u1: 3.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 0.0, v0: 13.0, u1: 1.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 1.0, v0: 12.0, u1: 2.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 2.0, v0: 12.0, u1: 3.0, v1: 13.0, rotation: 0 }, FaceUv { u0: 1.0, v0: 13.0, u1: 2.0, v1: 16.0, rotation: 0 }, FaceUv { u0: 3.0, v0: 13.0, u1: 4.0, v1: 16.0, rotation: 0 }] };
const CHIBI_LEG_RIGHT: ChibiBox = ChibiBox { off: [0.0,-2.4,0.0], size: [1.5,4.8,1.5], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 2.0, v0: 5.0, u1: 3.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 0.0, v0: 5.0, u1: 1.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 1.0, v0: 4.0, u1: 2.0, v1: 5.0, rotation: 0 }, FaceUv { u0: 2.0, v0: 4.0, u1: 3.0, v1: 5.0, rotation: 0 }, FaceUv { u0: 1.0, v0: 5.0, u1: 2.0, v1: 8.0, rotation: 0 }, FaceUv { u0: 3.0, v0: 5.0, u1: 4.0, v1: 8.0, rotation: 0 }] };
const CHIBI_LEG_RIGHT_LAYER: ChibiBox = ChibiBox { off: [0.0,-2.4,0.0], size: [1.7,5.0,1.7], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 2.0, v0: 9.0, u1: 3.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 0.0, v0: 9.0, u1: 1.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 1.0, v0: 8.0, u1: 2.0, v1: 9.0, rotation: 0 }, FaceUv { u0: 2.0, v0: 8.0, u1: 3.0, v1: 9.0, rotation: 0 }, FaceUv { u0: 1.0, v0: 9.0, u1: 2.0, v1: 12.0, rotation: 0 }, FaceUv { u0: 3.0, v0: 9.0, u1: 4.0, v1: 12.0, rotation: 0 }] };

#[derive(Clone)]
struct BoxPart {
    size: Vec3,
    uv: Vec2,
    transform: Mat4,
    inflate: f32,
    mirror: bool,
    keyboard: bool,
    cape: bool,
    /// Q 版模式：六个面各自独立的 UV，顺序同 `draw_box` 的 face 0..5
    /// （+x 左 / -x 右 / +y 顶 / -y 底 / +z 前 / -z 后）。`None` 表示用标准
    /// cuboid 展开（`uv` + 尺寸推导）。
    faces: Option<[FaceUv; 6]>,
    /// 只有头部正面带眼睛；眨眼时在这一块里落眼皮。
    eyes: Option<Eyes>,
    /// 睁眼程度，1 = 完全睁开，0 = 完全闭合。
    eyelid: f32,
    /// 瞳孔位移（面部像素），头部转向时让眼珠跟着看过去。
    pupil: Vec2,
}
fn parts(s: &Settings, skin: &Skin, pose: &Pose) -> Vec<BoxPart> {
    parts_with_cape(s, skin, pose, None)
}
fn parts_with_cape(
    s: &Settings,
    skin: &Skin,
    pose: &Pose,
    cape: Option<&Cape>,
) -> Vec<BoxPart> {
    if s.chibi {
        return parts_chibi(s, skin, pose, cape);
    }
    let custom = s.posture == "custom";
    let custom_body = if custom {
        Mat4::from_quat(degrees(&s.active_custom_pose().body))
    } else {
        Mat4::IDENTITY
    };
    let body = pose.body() * custom_body;
    let aw = if s.arm_model == "slim" && !skin.legacy {
        3.
    } else {
        4.
    };
    let mut v = Vec::with_capacity(15);
    let anchor = 6. * pose.sit;
    let squash = Mat4::from_translation(Vec3::Y * anchor)
        * Mat4::from_scale(Vec3::new(1., pose.squash, 1.))
        * Mat4::from_translation(-Vec3::Y * anchor);
    let mut part = |transform: Mat4,
                    size: Vec3,
                    u: f32,
                    w: f32,
                    outer: Vec2,
                    mirror: bool,
                    hat: bool,
                    eyes: bool| {
        v.push(BoxPart {
            size,
            uv: Vec2::new(u, w),
            transform,
            inflate: 0.,
            mirror,
            keyboard: false,
            cape: false,
            faces: None,
            eyes: if eyes && s.da_blink { skin.eyes } else { None },
            eyelid: pose.eyelid,
            pupil: if eyes { pose.pupil } else { Vec2::ZERO },
        });
        if s.outer_layer && (!skin.legacy || hat) {
            v.push(BoxPart {
                size,
                uv: outer,
                transform,
                inflate: if hat { 1. } else { 0.5 },
                mirror,
                keyboard: false,
                cape: false,
                faces: None,
                // 外层帽子是叠加在头部上的一层，眼皮由内层负责。
                eyes: None,
                eyelid: pose.eyelid,
                pupil: Vec2::ZERO,
            });
        }
    };
    part(
        body * Mat4::from_translation(Vec3::new(0., 6., 0.)),
        Vec3::new(8., 12., 4.),
        16.,
        16.,
        Vec2::new(16., 32.),
        false,
        false,
        false,
    );
    part(
        pose.torso_space()
            * custom_body
            * Mat4::from_translation(Vec3::new(0., 12., 0.))
            * Mat4::from_quat(pose.head_rotation)
            * Mat4::from_translation(Vec3::Y * 4.),
        Vec3::splat(8.),
        0.,
        0.,
        Vec2::new(32., 0.),
        false,
        true,
        true,
    );
    for side in 0..2 {
        let left = side == 0;
        let sign = if left { 1. } else { -1. };
        let tr = pose.torso_space() * Mat4::from_rotation_y(pose.body_yaw) * custom_body
            * Mat4::from_translation(Vec3::new(sign * (4. + aw / 2.) + pose.arm_slide,
                10. + sign * pose.arm_bob, 0.))
            * Mat4::from_quat(pose.arms[side])
            * Mat4::from_translation(Vec3::new(0., -4., 0.));
        let (u, w, outer) = if left && !skin.legacy {
            (32., 48., Vec2::new(48., 48.))
        } else {
            (40., 16., Vec2::new(40., 32.))
        };
        part(
            tr,
            Vec3::new(aw, 12., 4.),
            u,
            w,
            outer,
            left && skin.legacy,
            false,
            false,
        );
    }
    for side in 0..2 {
        let left = side == 0;
        let sign = if left { 1. } else { -1. };
        let tr = pose.base_body()
            * Mat4::from_translation(Vec3::new(
                sign * (2. + 0.25 * pose.sit) + pose.leg_slide,
                0.,
                0.,
            ))
            * Mat4::from_rotation_z(pose.leg_roll)
            * Mat4::from_quat({
                let base = rotation(
                    -std::f32::consts::PI * 0.48 * pose.sit - 0.24 * pose.crouch,
                    sign * 0.16 * pose.sit,
                    sign * 0.045 * pose.sit,
                );
                if custom {
                    base * degrees(s.active_custom_pose().row(4 + side))
                } else {
                    base
                }
            })
            * Mat4::from_translation(Vec3::new(0., -6., 0.));
        let (u, w, outer) = if left && !skin.legacy {
            (16., 48., Vec2::new(0., 48.))
        } else {
            (0., 16., Vec2::new(0., 32.))
        };
        part(
            tr,
            Vec3::new(4., 12., 4.),
            u,
            w,
            outer,
            left && skin.legacy,
            false,
            false,
        );
    }
    if cape.is_some() {
        // 10×16×1 的薄板挂在背后：UV 与方块面包裹一致，
        // 中间那一面正好落在官方纹理的 (1,1)..(11,17)。
        // 官方披风的图案画在“外壳”那一面，也就是 (-Z) 那一侧；
        // 方块 UV 里 -Z 面的起点是 u + 2d + w，取 u = -11 正好落在 (1,1)。
        v.push(BoxPart {
            size: Vec3::new(10., 16., 1.),
            uv: Vec2::new(-11., 0.),
            transform: body
                * Mat4::from_translation(Vec3::new(0., 3.6, -2.9))
                * Mat4::from_rotation_x(0.14),
            inflate: 0.,
            mirror: false,
            keyboard: false,
            cape: true,
            faces: None,
            eyes: None,
            eyelid: 1.,
            pupil: Vec2::ZERO,
        });
    }
    // Squash only the character about its seated support height, never the keyboard
    // or native window. This cannot accumulate into the user-selected scale.
    for part in &mut v {
        part.transform = Mat4::from_translation(Vec3::Y * pose.gesture_jump + pose.gesture_shake) * squash * part.transform;
    }
    if pose.sit > 0.4 {
        v.push(BoxPart {
            size: Vec3::new(19.6, 0.65, 7.8),
            uv: Vec2::ZERO,
            transform: Mat4::from_translation(Vec3::new(0., 10.2, 9.4))
                * Mat4::from_scale(Vec3::splat(pose.sit))
                * Mat4::from_translation(Vec3::new(0., -0.3, 0.)),
            inflate: 0.,
            mirror: false,
            keyboard: true,
            cape: false,
            faces: None,
            eyes: None,
            eyelid: 1.,
            pupil: Vec2::ZERO,
        });
    }
    let limb_layers = if s.outer_layer && !skin.legacy { 2 } else { 1 };
    let head_layers = if s.outer_layer { 2 } else { 1 };
    cover_cheeks(&mut v, limb_layers + head_layers - 1,
        limb_layers + head_layers, limb_layers, pose.cheek_cover);
    let global = model_transform(s);
    for part in &mut v {
        part.transform = global * part.transform;
    }
    // Three.js draws opaque first and orders translucent meshes by camera-space center.
    v.sort_by(|a, b| {
        (a.inflate > 0.).cmp(&(b.inflate > 0.)).then_with(|| {
            if a.inflate > 0. {
                let az = project(a.transform.transform_point3(Vec3::ZERO), 1., 1.).z;
                let bz = project(b.transform.transform_point3(Vec3::ZERO), 1., 1.).z;
                bz.total_cmp(&az)
            } else {
                std::cmp::Ordering::Equal
            }
        })
    });
    v
}

/// Q 版(图腾玩偶)模式：用 Q 版 cuboid + 标准皮肤贴图，挂到现有骨骼动画上。
/// 复用 Pose 的角度，但髋高度、身体尺寸、肩/髋 pivot 按 Q 版比例。
fn chibi_typing_rotation(s: &Settings, pose: &Pose, side: usize, key: Option<Vec2>, active: bool) -> Quat {
    let sign = if side == 0 { 1. } else { -1. };
    let custom = if s.posture == "custom" { Mat4::from_quat(degrees(&s.active_custom_pose().body)) } else { Mat4::IDENTITY };
    let parent = Mat4::from_translation(Vec3::new(0., 4.8 - 3. * pose.sit + pose.breathing_bob, 0.))
        * custom * Mat4::from_rotation_y(pose.body_yaw);
    let shoulder = parent.transform_point3(Vec3::new(sign * 2.35, 3.7, 0.));
    // Solve a four-unit arm against the board plane. Both board and character
    // receive the same body-slider transform; undo only the typing squash here.
    let y = 3. * pose.sit + (3.4 + if active { 0. } else { 0.4 } - 3. * pose.sit) / pose.squash.max(0.1);
    let x = key.map(|k| k.x * 0.4).unwrap_or(sign * 1.7);
    let dy = y - shoulder.y;
    let z = 2.8 + key.map(|k| k.y * 0.18).unwrap_or(0.);
    let horizontal = Vec2::new(x - shoulder.x, z - shoulder.z).normalize_or_zero()
        * (16. - dy * dy).max(0.1).sqrt();
    let target = Vec3::new(shoulder.x + horizontal.x, y, shoulder.z + horizontal.y);
    Quat::from_rotation_arc(-Vec3::Y, parent.inverse().transform_vector3(target - shoulder).normalize())
}
fn parts_chibi(s: &Settings, skin: &Skin, pose: &Pose, cape: Option<&Cape>) -> Vec<BoxPart> {
    let slim = s.arm_model == "slim" && !skin.legacy;
    let body_scale = s.chibi_body_scale / 100.;
    let thickness = 1.;
    let arm_w = 1.2;
    let custom = s.posture == "custom";
    let custom_body = if custom {
        Mat4::from_quat(degrees(&s.active_custom_pose().body))
    } else {
        Mat4::IDENTITY
    };
    // Q 版身体：髋在 Y=4（站姿，脚在 Y=0）。坐姿下降、潜行俯身。
    let base = Mat4::from_translation(Vec3::new(
        0.,
        4.8 - 3. * pose.sit - 1.0 * pose.crouch,
        -0.6 * pose.crouch,
    )) * Mat4::from_rotation_x(0.34 * pose.crouch);
    let body = Mat4::from_translation(Vec3::Y * (pose.body_bob+pose.breathing_bob))
        * base
        * Mat4::from_translation(Vec3::Y * 3.7)
        * Mat4::from_rotation_z(pose.body_roll)
        * Mat4::from_translation(-Vec3::Y * 3.7)
        * Mat4::from_rotation_y(pose.body_yaw)
        * custom_body;
    let torso = Mat4::from_translation(Vec3::Y * (pose.body_bob+pose.breathing_bob))
        * base
        * custom_body;

    let mut v = Vec::with_capacity(14);
    let push = |v: &mut Vec<BoxPart>, cb: &ChibiBox, geometry: &ChibiBox, transform: Mat4, layer: bool, eyes: bool, original_size: Vec3| {
        let t = transform
            * Mat4::from_quat(degrees(&geometry.rot))
            * Mat4::from_translation(Vec3::from_array(geometry.off));
        let inner_size = Vec3::from_array(geometry.size);
        // Match the normal model's shell ratio on each axis. Keep inflate as
        // the renderer's translucent-pass marker, accounting for it exactly
        // once in draw_box. Layer cuboids supply UVs, never extra enlargement.
        let inflate = if layer { 0.5 } else { 0. };
        let expansion = if layer {
            inner_size / original_size * if original_size == Vec3::splat(8.) { 1. } else { 0.5 }
        } else { Vec3::ZERO };
        v.push(BoxPart {
            size: inner_size + expansion - Vec3::splat(inflate),
            uv: Vec2::ZERO,
            transform: t,
            inflate,
            mirror: false,
            keyboard: false,
            cape: false,
            faces: Some(cb.faces),
            eyes: if eyes && s.da_blink { skin.eyes } else { None },
            eyelid: pose.eyelid,
            pupil: if eyes { pose.pupil } else { Vec2::ZERO },
        });
    };

    // 头（颈在身体顶部 Y=6）
    let head = torso
        * Mat4::from_translation(Vec3::new(0., 3.7, 0.))
        * Mat4::from_quat(pose.head_rotation);
    push(&mut v, &CHIBI_HEAD, &CHIBI_HEAD, head, false, true, Vec3::splat(8.));
    if s.outer_layer {
        push(&mut v, &CHIBI_HAT, &CHIBI_HEAD, head, true, false, Vec3::splat(8.));
    }

    let anchor = 3. * pose.sit;
    let squash = Mat4::from_translation(Vec3::Y * anchor)
        * Mat4::from_scale(Vec3::new(1., pose.squash, 1.))
        * Mat4::from_translation(-Vec3::Y * anchor);
    let body_scale_mat = Mat4::from_translation(Vec3::Y * anchor)
        * Mat4::from_scale(Vec3::splat(body_scale))
        * Mat4::from_translation(-Vec3::Y * anchor);
    let neck = (squash * head).transform_point3(Vec3::ZERO);
    let neck_delta = body_scale_mat.transform_point3(neck) - neck;

    // 身体（中心在 Y=3）
    let body_t = body * Mat4::from_translation(Vec3::new(0., 1.85, 0.));
    push(&mut v, &CHIBI_BODY, &CHIBI_BODY, body_t, false, false, Vec3::new(8., 12., 4.));
    if s.outer_layer {
        push(&mut v, &CHIBI_BODY_LAYER, &CHIBI_BODY, body_t, true, false, Vec3::new(8., 12., 4.));
    }

    // 手臂（肩在 Y=5 两侧）
    for side in 0..2 {
        let left = side == 0;
        let sign = if left { 1. } else { -1. };
        // Shoulder pivots belong to the animated torso. Normal-body slide/bob
        // offsets are not valid for this much shorter skeleton.
        let shoulder_space = if pose.dance > 0. { body } else { torso * Mat4::from_rotation_y(pose.body_yaw) };
        let arm = shoulder_space
            * Mat4::from_translation(Vec3::new(sign * (1.75 + arm_w / 2.), 3.7, 0.))
            * Mat4::from_quat(pose.arms[side])
            * Mat4::from_scale(Vec3::new(thickness, 1., thickness));
        let (inner, outer) = if left {
            if slim {
                (&CHIBI_ARM_LEFT_NARROW, &CHIBI_ARM_LEFT_LAYER_NARROW)
            } else {
                (&CHIBI_ARM_LEFT_WIDE, &CHIBI_ARM_LEFT_LAYER_WIDE)
            }
        } else if slim {
            (&CHIBI_ARM_RIGHT_NARROW, &CHIBI_ARM_RIGHT_LAYER_NARROW)
        } else {
            (&CHIBI_ARM_RIGHT_WIDE, &CHIBI_ARM_RIGHT_LAYER_WIDE)
        };
        let original_size = Vec3::new(if slim { 3. } else { 4. }, 12., 4.);
        push(&mut v, inner, inner, arm, false, false, original_size);
        if s.outer_layer {
            push(&mut v, outer, inner, arm, true, false, original_size);
        }
    }

    // 腿（髋在 Y=0 两侧，坐姿/潜行摆动）
    for side in 0..2 {
        let left = side == 0;
        let sign = if left { 1. } else { -1. };
        let leg_rot = if custom {
            degrees(&s.active_custom_pose().row(4 + side))
        } else {
            // 站姿腿直立；坐姿把腿前摆约 90°（变成向前伸），潜行收腿。
            rotation(
                -std::f32::consts::FRAC_PI_2 * pose.sit - 0.24 * pose.crouch,
                sign * 0.05 * pose.sit,
                0.,
            )
        };
        // Anchor each hip to the actual torso bottom, rather than approximating
        // it with the normal model's 12-unit lateral slide. Keep the leg's
        // independent counter-rotation, so the dance does not cancel itself.
        let hip_space = if pose.dance > 0. { body } else { base };
        let hip = hip_space.transform_point3(Vec3::new(sign * 0.75 * thickness, 0., 0.));
        let leg = Mat4::from_translation(hip)
            * Mat4::from_rotation_x(0.34 * pose.crouch)
            * Mat4::from_rotation_z(pose.leg_roll)
            * Mat4::from_quat(leg_rot)
            * Mat4::from_scale(Vec3::new(thickness, 1., thickness));
        let (inner, outer) = if left {
            (&CHIBI_LEG_LEFT, &CHIBI_LEG_LEFT_LAYER)
        } else {
            (&CHIBI_LEG_RIGHT, &CHIBI_LEG_RIGHT_LAYER)
        };
        push(&mut v, inner, inner, leg, false, false, Vec3::new(4., 12., 4.));
        if s.outer_layer {
            push(&mut v, outer, inner, leg, true, false, Vec3::new(4., 12., 4.));
        }
    }

    // Squash 只作用在角色上（锚点按 Q 版支撑高度）。
    if cape.is_some() {
        // Keep atlas dimensions unchanged; scale geometry with the torso.
        // All subsequent character transforms apply to the attached cape too.
        v.push(BoxPart {
            size: Vec3::new(10., 16., 1.),
            uv: Vec2::new(-11., 0.),
            transform: body
                * Mat4::from_translation(Vec3::new(0., 3.6 * (3.7 / 12.), -2.9 * (2.5 / 4.)))
                * Mat4::from_scale(Vec3::new(3.5 / 8., 3.7 / 12., 2.5 / 4.))
                * Mat4::from_rotation_x(0.14),
            inflate: 0., mirror: false, keyboard: false, cape: true,
            faces: None, eyes: None, eyelid: 1., pupil: Vec2::ZERO,
        });
    }
    for part in &mut v {
        part.transform = squash * part.transform;
    }
    // Scale body/limbs together about the seated support; keep head dimensions.
    let head_count = if s.outer_layer { 2 } else { 1 };
    for (index, part) in v.iter_mut().enumerate() {
        part.transform = if index < head_count {
            Mat4::from_translation(neck_delta) * part.transform
        } else { body_scale_mat * part.transform };
    }
    // Q 版整体放大（脚仍在 Y=0，头顶约 19.3，缩放后约 29）。
    let chibi_scale = Mat4::from_scale(Vec3::splat(1.5));
    for part in &mut v {
        part.transform = Mat4::from_translation((Vec3::Y * pose.gesture_jump + pose.gesture_shake) * body_scale * 0.6) * chibi_scale * part.transform;
    }
    // 坐姿打字键盘：和标准人形同一套尺寸/渐入，放在 Q 版坐姿身体前方。
    if pose.sit > 0.4 {
        v.push(BoxPart {
            size: Vec3::new(9., 0.4, 2.4),
            uv: Vec2::ZERO,
            transform: chibi_scale * body_scale_mat * Mat4::from_translation(Vec3::new(0., 3.2, 2.8))
                * Mat4::from_scale(Vec3::splat(pose.sit))
                ,
            inflate: 0.,
            mirror: false,
            keyboard: true,
            cape: false,
            faces: None,
            eyes: None,
            eyelid: 1.,
            pupil: Vec2::ZERO,
        });
    }
    cover_cheeks(&mut v, head_count - 1, head_count * 2, head_count, pose.cheek_cover);
    let global = model_transform(s);
    for part in &mut v {
        part.transform = global * part.transform;
    }
    v.sort_by(|a, b| {
        (a.inflate > 0.).cmp(&(b.inflate > 0.)).then_with(|| {
            if a.inflate > 0. {
                let az = project(a.transform.transform_point3(Vec3::ZERO), 1., 1.).z;
                let bz = project(b.transform.transform_point3(Vec3::ZERO), 1., 1.).z;
                bz.total_cmp(&az)
            } else {
                std::cmp::Ordering::Equal
            }
        })
    });
    v
}
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    depth: Vec<f32>,
    pub bounds: [i32; 4],
    keyboard_color: [u8; 3],
}
impl Frame {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            width: w,
            height: h,
            pixels: vec![0; w * h * 4],
            depth: vec![f32::INFINITY; w * h],
            bounds: [0; 4],
            keyboard_color: [131, 135, 138],
        }
    }
    pub fn draw(&mut self, s: &Settings, skin: &Skin, pose: &Pose) {
        self.draw_ext(s, skin, None, pose);
    }
    /// 带披风的绘制；披风只在正版账号取到的时候才传进来。
    pub fn draw_ext(&mut self, s: &Settings, skin: &Skin, cape: Option<&Cape>, pose: &Pose) {
        let rgb = u32::from_str_radix(s.keyboard_color.trim_start_matches('#'), 16).unwrap_or(0x83878A);
        self.keyboard_color = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
        self.pixels.fill(0);
        self.depth.fill(f32::INFINITY);
        self.bounds = [self.width as i32, self.height as i32, 0, 0];
        let cape = cape.filter(|_| s.show_cape);
        for part in parts_with_cape(s, skin, pose, cape) {
            if part.cape {
                if let Some(c) = cape {
                    self.draw_box(&part, Tex::of(&c.image, &c.colors));
                }
            } else {
                self.draw_box(&part, Tex::of(&skin.image, &skin.colors));
            }
        }
        self.draw_effects(s,skin,pose);
    }
    fn draw_box(&mut self, b: &BoxPart, skin: Tex) {
        let Vec3 { x: w, y: h, z: d } = b.size;
        let u = b.uv.x;
        let v = b.uv.y;
        let regions = [
            // +X is the character's left side. The unfolded PNG strip is
            // character-right / front / character-left / back, not BoxGeometry order.
            (u + d + w, v + d, d, h),
            (u, v + d, d, h),
            (u + d, v, w, d),
            (u + d + w, v, w, d),
            (u + d, v + d, w, h),
            (u + 2. * d + w, v + d, w, h),
        ];
        let sz = b.size + Vec3::splat(b.inflate);
        let planes = [
            (2, 1, 0, -1., -1., sz.z, sz.y, sz.x),
            (2, 1, 0, 1., -1., sz.z, sz.y, -sz.x),
            (0, 2, 1, 1., 1., sz.x, sz.z, sz.y),
            (0, 2, 1, 1., -1., sz.x, sz.z, -sz.y),
            (0, 1, 2, 1., -1., sz.x, sz.y, sz.z),
            (0, 1, 2, -1., -1., sz.x, sz.y, -sz.z),
        ];
        for (f, &(uax, vax, wax, ud, vd, ww, hh, dd)) in planes.iter().enumerate() {
            let mut pos = [Vec3::ZERO; 4];
            let mut uv = [Vec2::ZERO; 4];
            let (x, y, tw, th) = regions[f];
            // Q 版每面独立 UV：把 [u0,v0,u1,v1]（0..16 单位）按 rotation 排到
            // 四个顶点上，再乘 4 转成 64 基准贴图坐标。
            // 坐标映射是绕 Y 轴 180° 的镜像，会翻转南北东西面的 u 轴（左右），
            // 使这些面上的 UV 旋转方向反转（90↔270）；顶底面 u、v 都翻转，方向不变。
            let custom = b.faces.map(|fs| {
                let fu = fs[f];
                let corners = [
                    (fu.u0, fu.v0),
                    (fu.u1, fu.v0),
                    (fu.u0, fu.v1),
                    (fu.u1, fu.v1),
                ];
                let rot = if f == 2 || f == 3 {
                    fu.rotation
                } else {
                    match fu.rotation {
                        90 => 270,
                        270 => 90,
                        r => r,
                    }
                };
                let order: [usize; 4] = match rot {
                    90 => [2, 0, 3, 1],
                    180 => [3, 2, 1, 0],
                    270 => [1, 3, 0, 2],
                    _ => [0, 1, 2, 3],
                };
                order.map(|i| Vec2::new(corners[i].0 * 4., corners[i].1 * 4.))
            });
            for iy in 0..2 {
                for ix in 0..2 {
                    let i = iy * 2 + ix;
                    let mut a = Vec3::ZERO;
                    a[uax] = (ix as f32 * ww - ww / 2.) * ud;
                    a[vax] = (iy as f32 * hh - hh / 2.) * vd;
                    a[wax] = dd / 2.;
                    if b.mirror {
                        a.x = -a.x;
                    }
                    pos[i] = project(
                        b.transform.transform_point3(a),
                        self.width as f32,
                        self.height as f32,
                    );
                    uv[i] = match custom {
                        Some(uvs) => uvs[i],
                        None => Vec2::new(
                            x + ix as f32 * tw,
                            y + if f == 3 {
                                (1 - iy) as f32 * th
                            } else {
                                iy as f32 * th
                            },
                        ),
                    };
                }
            }
            for ids in [[0, 2, 1], [2, 3, 1]] {
                let area = edge(pos[ids[0]], pos[ids[1]], pos[ids[2]]);
                if b.inflate == 0. && ((area >= 0.) != b.mirror) {
                    continue;
                }
                self.triangle(
                    [pos[ids[0]], pos[ids[1]], pos[ids[2]]],
                    [uv[ids[0]], uv[ids[1]], uv[ids[2]]],
                    f,
                    b,
                    skin,
                );
            }
        }
        if let Some(eyes) = b.eyes {
            if b.inflate == 0. {
                for eye in [eyes.left, eyes.right] {
                    self.eyelid(b, eye, skin);
                    self.pupil(b, eye, skin);
                }
            }
        }
    }
    /// 眼珠：把眼睛贴图里的瞳孔那一小段像素画到它的新位置上。
    /// 资源包用 `ctrl_*_pupil.tx/ty` 平移瞳孔骨骼，眼白和眼皮不动；
    /// 这里同样只搬动瞳孔像素，`b.pupil` 是位移量（单位：面部像素）。
    fn pupil(&mut self, b: &BoxPart, eye: Eye, skin: Tex) {
        if b.pupil.length_squared() < 1e-6 {
            return;
        }
        let sz = b.size + Vec3::splat(b.inflate);
        // 比脸再靠前一点，但仍在眼皮之下：眨眼时瞳孔被眼皮盖住。
        let z = sz.z / 2. + 0.012;
        let (px0, px1) = eye.pupil;
        let x0 = px0 as f32 + b.pupil.x - 4.;
        let x1 = px1 as f32 + 1. + b.pupil.x - 4.;
        let y0 = eye.y0 as f32 + b.pupil.y;
        let y1 = eye.y0 as f32 + eye.rows as f32 + b.pupil.y;
        let top = 4. - y0;
        let bottom = 4. - y1;
        // 位移不到一整格时用贴图上的小数坐标采样，移动才连续不跳格。
        let (u0, u1) = (8. + px0 as f32 + b.pupil.x, 8. + px1 as f32 + 1. + b.pupil.x);
        let (v0, v1) = (8. + y0, 8. + y1);
        let corners = [
            (Vec3::new(x0, top, z), Vec2::new(u0, v0)),
            (Vec3::new(x1, top, z), Vec2::new(u1, v0)),
            (Vec3::new(x0, bottom, z), Vec2::new(u0, v1)),
            (Vec3::new(x1, bottom, z), Vec2::new(u1, v1)),
        ];
        let pos = corners.map(|(p, _)| {
            project(
                b.transform.transform_point3(p),
                self.width as f32,
                self.height as f32,
            )
        });
        let uv = corners.map(|(_, t)| t);
        for ids in [[0, 2, 1], [2, 3, 1]] {
            self.triangle(
                [pos[ids[0]], pos[ids[1]], pos[ids[2]]],
                [uv[ids[0]], uv[ids[1]], uv[ids[2]]],
                4,
                b,
                skin,
            );
        }
    }
    /// 眨眼的眼皮：材质包的 `right_blink` / `left_blink` 骨骼用 `sy` 从 0 长到 1，
    /// 把眼睛那一行盖住；它采样的就是玩家画在眼睛正上方的那一行像素。
    /// 这里在头部正面加一层同样宽度的薄片，UV 指向那块眼皮像素，
    /// 所以闭眼时看到的是玩家自己画在皮肤上的眼皮色，而不是被拉伸的眼睛。
    fn eyelid(&mut self, b: &BoxPart, eye: Eye, skin: Tex) {
        let closed = 1. - b.eyelid;
        if closed <= 0.002 {
            return;
        }
        let sz = b.size + Vec3::splat(b.inflate);
        // 面部正面在头盒 +Z 一侧；眼皮比其他面靠前一丁点，避免与脸部抢占深度。
        let z = sz.z / 2. + 0.02;
        let x0 = (eye.x0 as f32 - 4.) * sz.x / 8.;
        let x1 = (eye.x1 as f32 + 1. - 4.) * sz.x / 8.;
        let top = (4. - eye.y0 as f32) * sz.y / 8.;
        // 眼皮从上边缘向下长满眼睛所在的那一行（材质包的 blink.sy 0→1）。
        let bottom = top - eye.rows as f32 * closed * sz.y / 8.;
        // UV 直接取识别格里的眼皮像素；识别格坐标本身就是贴图坐标，
        // 不能再叠加脸部 8×8 的偏移。
        let lx0 = eye.lid.0 as f32;
        let lx1 = eye.lid.2 as f32 + 1.;
        let ly = eye.lid.1 as f32 + 0.5;
        let corners = [
            (Vec3::new(x0, top, z), Vec2::new(lx0, ly)),
            (Vec3::new(x1, top, z), Vec2::new(lx1, ly)),
            (Vec3::new(x0, bottom, z), Vec2::new(lx0, ly)),
            (Vec3::new(x1, bottom, z), Vec2::new(lx1, ly)),
        ];
        let pos = corners.map(|(p, _)| {
            project(
                b.transform.transform_point3(p),
                self.width as f32,
                self.height as f32,
            )
        });
        let uv = corners.map(|(_, t)| t);
        for ids in [[0, 2, 1], [2, 3, 1]] {
            self.triangle(
                [pos[ids[0]], pos[ids[1]], pos[ids[2]]],
                [uv[ids[0]], uv[ids[1]], uv[ids[2]]],
                4,
                b,
                skin,
            );
        }
    }
    fn triangle(&mut self, p: [Vec3; 3], uv: [Vec2; 3], face: usize, b: &BoxPart, skin: Tex) {
        let area = edge(p[0], p[1], p[2]);
        if area.abs() < 0.0001 {
            return;
        }
        let minx = p
            .iter()
            .map(|p| p.x)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.) as usize;
        let maxx = p
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.width as f32) as usize;
        let miny = p
            .iter()
            .map(|p| p.y)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.) as usize;
        let maxy = p
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(self.height as f32) as usize;
        let texscale = skin.image.width() as f32 / 64.;
        let shades = [0.80f32, 0.80, 1., 0.62, 0.94, 0.86];
        let gray = self.keyboard_color.map(|c| {
            let l = (((c as f32 / 255. + 0.055) / 1.055).powf(2.4) * shades[face]).max(0.);
            ((1.055 * l.powf(1. / 2.4) - 0.055) * 255. + 0.5) as u8
        });
        let inv_area = area.recip();
        let ax = -(p[2].y - p[1].y) * inv_area;
        let ay = (p[2].x - p[1].x) * inv_area;
        let cx = -(p[1].y - p[0].y) * inv_area;
        let cy = (p[1].x - p[0].x) * inv_area;
        let origin = Vec3::new(minx as f32 + 0.5, miny as f32 + 0.5, 0.);
        let a0 = edge(p[1], p[2], origin) * inv_area;
        let c0 = edge(p[0], p[1], origin) * inv_area;
        let za = p[0].z - p[1].z;
        let zc = p[2].z - p[1].z;
        let tex = uv.map(|t| t * texscale);
        let ta = tex[0] - tex[1];
        let tc = tex[2] - tex[1];
        // Fixed-point, half-open coverage gives shared triangle edges exactly one
        // owner: no transparent cracks or twice-blended outer-layer diagonals.
        let points = p.map(|v| {
            (
                (v.x as f64 * 256.).round() as i64,
                (v.y as f64 * 256.).round() as i64,
            )
        });
        let edges = [(0, 1), (1, 2), (2, 0)].map(|(i, j)| {
            let (s, e) = if area > 0. {
                (points[i], points[j])
            } else {
                (points[j], points[i])
            };
            let dx = e.0 - s.0;
            let dy = e.1 - s.1;
            let bias = if dy < 0 || (dy == 0 && dx > 0) { 0 } else { -1 };
            (
                dx,
                dy,
                dx * (miny as i64 * 256 + 128 - s.1) - dy * (minx as i64 * 256 + 128 - s.0) + bias,
            )
        });
        for y in miny..maxy {
            let row_a = a0 + (y - miny) as f32 * ay;
            let row_c = c0 + (y - miny) as f32 * cy;
            let coverage = edges.map(|(dx, _, start)| start + dx * (y - miny) as i64 * 256);
            for x in minx..maxx {
                if (0..3).any(|e| coverage[e] - edges[e].1 * (x - minx) as i64 * 256 < 0) {
                    continue;
                }
                let a = row_a + (x - minx) as f32 * ax;
                let c = row_c + (x - minx) as f32 * cx;
                let z = p[1].z + za * a + zc * c;
                let i = y * self.width + x;
                if z >= self.depth[i] {
                    continue;
                }
                let (color, alpha) = if b.keyboard {
                    (gray, 255)
                } else {
                    let tx = tex[1].x + ta.x * a + tc.x * c;
                    let ty = tex[1].y + ta.y * a + tc.y * c;
                    let xx = (tx as u32).min(skin.image.width() - 1);
                    let yy = (ty as u32).min(skin.image.height() - 1);
                    let ti = (yy * skin.image.width() + xx) as usize;
                    (skin.colors[ti][face], skin.image.get_pixel(xx, yy)[3])
                };
                if alpha < 16 {
                    continue;
                }
                let off = i * 4;
                if alpha == 255 {
                    self.pixels[off..off + 4].copy_from_slice(&[color[2], color[1], color[0], 255]);
                } else {
                    let aa = alpha as u32;
                    let inv = 255 - aa;
                    let channels = [color[2], color[1], color[0]];
                    for ch in 0..3 {
                        self.pixels[off + ch] =
                            ((channels[ch] as u32 * aa + self.pixels[off + ch] as u32 * inv + 127)
                                / 255) as u8;
                    }
                    self.pixels[off + 3] =
                        (aa + (self.pixels[off + 3] as u32 * inv + 127) / 255) as u8;
                }
                self.depth[i] = z;
                self.bounds[0] = self.bounds[0].min(x as i32);
                self.bounds[1] = self.bounds[1].min(y as i32);
                self.bounds[2] = self.bounds[2].max(x as i32 + 1);
                self.bounds[3] = self.bounds[3].max(y as i32 + 1);
            }
        }
    }
    pub fn rgba(&self) -> Vec<u8> {
        self.pixels
            .chunks_exact(4)
            .flat_map(|p| {
                let a = p[3] as u32;
                let c = |v: u8| {
                    if a == 0 {
                        0
                    } else {
                        (v as u32 * 255 / a).min(255) as u8
                    }
                };
                [c(p[2]), c(p[1]), c(p[0]), p[3]]
            })
            .collect()
    }
}
fn edge(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn breathing_runs_immediately_through_actions_and_preserves_custom_pose() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for posture in ["standing","sitting","custom"] {for base in ["standing","sitting"] {for chibi in [false,true] {
            let mut s=Settings::default();s.posture=posture.into();s.custom_base=base.into();
            s.track_mouse=false;s.breathing_animation=true;s.tease_enabled=true;s.chibi=chibi;
            s.custom_pose.left_arm=[20.,40.,30.];s.custom_sitting_pose.right_arm=[-25.,15.,45.];
            let saved=serde_json::to_value(&s).unwrap();let mut p=Pose::new(&s);
            tick(&mut p,&s,&skin,0.01,false);assert!(p.breathing_weight>0.);
            for i in 1..240 {tick(&mut p,&s,&skin,i as f32/120.,false);}
            assert!(p.breathing_weight>0.98);
            p.key(65,true,2.,&s);tick(&mut p,&s,&skin,2.1,true);
            assert!(p.breathing_weight>0.98);assert!(p.breathing_bob.abs()>0.001);
            p.click(2.2,false);tick(&mut p,&s,&skin,2.25,true);assert!(p.breathing_weight>0.98);
            p.feed(2.3);tick(&mut p,&s,&skin,2.4,true);assert!(p.breathing_weight>0.98);
            p.tease(2.5);tick(&mut p,&s,&skin,2.6,true);assert!(p.breathing_weight>0.98);
            assert_eq!(serde_json::to_value(&s).unwrap(),saved);
            s.da_dance=true;tick(&mut p,&s,&skin,3.2,false);assert!(p.breathing_weight>0.98);
            p.key(65,false,3.3,&s);s.da_dance=false;s.breathing_animation=false;
            for i in 0..360 {tick(&mut p,&s,&skin,4.+i as f32/120.,false);}
            assert_eq!(p.breathing_weight,0.);assert_eq!(p.breathing_bob,0.);
            let mut rest=Pose::new(&s);
            for i in 0..360 {tick(&mut rest,&s,&skin,4.+i as f32/120.,false);}
            for side in 0..2 {assert!(p.arms[side].angle_between(rest.arms[side])<0.001,"{posture} {base} {chibi} {side}: {:?}",p.arms[side].angle_between(rest.arms[side]));}
        }}}
    }
    #[test]
    fn tease_interrupts_actions_and_returns_to_live_held_pose() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for posture in ["standing","sitting","custom"] {for chibi in [false,true] {for kind in 0..3 {
            let mut s=Settings::default();s.posture=posture.into();s.custom_base="sitting".into();
            s.chibi=chibi;s.tease_enabled=true;s.track_mouse=false;
            s.custom_sitting_pose.left_arm=[-20.,30.,15.];
            let mut p=Pose::new(&s);p.key(65,true,0.,&s);
            for i in 0..120 {tick(&mut p,&s,&skin,i as f32/120.,false);}
            let held=p.held_codes();let rest=p.arms;
            p.feed(1.);tick(&mut p,&s,&skin,1.2,false);
            let captured=p.arms;p.tease(1.2);p.tease_kind=kind;
            assert!(p.feed_start.is_none());assert!(p.swing_start.is_none());
            tick(&mut p,&s,&skin,1.2,false);
            for side in 0..2 {assert!(p.arms[side].angle_between(captured[side])<0.001);}
            p.click(1.3,true);assert!(p.swing_start.is_none());
            p.feed(1.3);assert!(p.feed_start.is_none());assert!(p.tease_start.is_some());
            for i in 1..61 {tick(&mut p,&s,&skin,1.2+i as f32/120.,false);}
            tick(&mut p,&s,&skin,1.71,false);
            assert!(p.tease_start.is_none());assert_eq!(p.held_codes(),held);
            for side in 0..2 {assert!(p.arms[side].angle_between(rest[side])<0.001);}
            p.click(2.,false);assert!(p.swing_start.is_some());
            p.tease(2.1);assert!(p.swing_start.is_none());
        }}}
    }
    #[test]
    fn letters_choose_stable_cape_palette_colors() {
        let mut s=Settings::default();s.posture="sitting".into();s.typing_letters=true;
        let mut p=Pose::new(&s);p.random_state=123456;
        for i in 0..40 {p.key(65,true,i as f32*0.01,&s);p.key(65,false,i as f32*0.01+0.001,&s);}
        assert_eq!(p.letters.len(),40);
        assert!(p.letters.iter().all(|l|effects::CANDY_COLORS.contains(&l.color)));
        assert!(p.letters.iter().any(|l|l.color!=p.letters[0].color));
        let colors:Vec<_>=p.letters.iter().map(|l|l.color).collect();
        p.key(66,true,0.5,&s);p.key(66,true,0.51,&s);
        assert_eq!(p.letters.len(),41);
        assert_eq!(p.letters[..40].iter().map(|l|l.color).collect::<Vec<_>>(),colors);
    }

    #[test]
    fn every_click_restarts_swing_without_a_queued_tail() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for (posture, base) in [
            ("standing", "standing"),
            ("sitting", "standing"),
            ("custom", "standing"),
            ("custom", "sitting"),
        ] {
            for left in [false, true] {
                let mut s = Settings::default();
                s.posture = posture.into();
                s.custom_base = base.into();
                s.track_mouse = false;
                let mut p = Pose::new(&s);
                let vk = if left { 80 } else { 81 };
                p.key(vk, true, 0., &s);
                for i in 0..120 {
                    tick(&mut p, &s, &skin, i as f32 / 120., false);
                }
                let side = if left { 0 } else { 1 };
                let rest = p.arms[side];
                p.click(1., left);
                tick(&mut p, &s, &skin, 1.12, false);
                assert!(p.swing_progress > 0.39);
                for i in 0..12 {
                    let now = 1.13 + i as f32 * 0.04;
                    p.click(now, left);
                    assert_eq!(p.swing_progress, 0.);
                    assert_eq!(p.swing_start, Some(now));
                    assert_eq!(p.swing_from, p.typing_arms[side]);
                    tick(&mut p, &s, &skin, now + 0.01, false);
                    assert!((p.swing_progress - 1. / 30.).abs() < 0.0001);
                }
                let end = 1.13 + 11. * 0.04 + SWING_SECONDS + 0.001;
                tick(&mut p, &s, &skin, end, false);
                assert!(p.swing_start.is_none());
                assert_eq!(p.swing_progress, 0.);
                assert!(p.arms[side].angle_between(rest) < 0.001);
                if s.is_sitting() {
                    assert_eq!(p.held_codes(), vec![vk]);
                }
            }
        }
    }
    #[test]
    fn cape_renders_from_the_official_texture_region() {
        use image::{ImageFormat, Rgba, RgbaImage};
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        // 官方披风纹理：只有 (1,1)-(11,17) 是有效面，别处全透明。
        let mut tex = RgbaImage::from_pixel(64, 32, Rgba([0, 0, 0, 0]));
        for y in 1..17 {
            for x in 1..11 {
                tex.put_pixel(x, y, Rgba([220, 30, 30, 255]));
            }
        }
        let mut png = std::io::Cursor::new(Vec::new());
        tex.write_to(&mut png, ImageFormat::Png).unwrap();
        let cape = Cape::load(png.get_ref()).unwrap();

        let mut s = Settings::default();
        s.posture = "standing".into();
        let red = |f: &Frame| {
            f.rgba()
                .chunks_exact(4)
                .filter(|c| c[3] > 0 && c[0] > 120 && c[1] < 90 && c[2] < 90)
                .count()
        };

        // 正面：披风在身后，几乎看不到。
        let mut front = Frame::new(360, 420);
        front.draw_ext(&s, &skin, Some(&cape), &Pose::new(&s));
        assert!(red(&front) < 60, "正面不该大块露出披风");

        // 背面：整片披风应该盖住后背。
        s.model_rotation = [0., 180., 0.];
        let back_pose = Pose::new(&s);
        let mut back = Frame::new(360, 420);
        back.draw_ext(&s, &skin, Some(&cape), &back_pose);
        assert!(red(&back) > 2000, "转到背面应该看到整片披风");

        // 关掉开关就不画。
        s.show_cape = false;
        let mut off = Frame::new(360, 420);
        off.draw_ext(&s, &skin, Some(&cape), &back_pose);
        assert_eq!(red(&off), 0, "关掉开关就不该画披风");

        let _ = image::save_buffer(
            std::env::temp_dir().join("cape-preview.png"),
            &back.rgba(),
            360,
            420,
            image::ColorType::Rgba8,
        );
    }

    fn head_axes(s: &Settings, p: &Pose, skin: &Skin) -> (Vec3, Vec3) {
        let boxes = parts(s, skin, p);
        let head = boxes
            .iter()
            .find(|b| b.uv == Vec2::ZERO && b.size == Vec3::splat(8.) && b.inflate == 0.)
            .unwrap();
        (
            head.transform.transform_vector3(Vec3::Z).normalize(),
            head.transform.transform_vector3(Vec3::Y).normalize(),
        )
    }
    #[test]
    fn tracked_head_stays_screen_aligned_after_all_parent_rotations() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut s = Settings::default();
        s.posture = "custom".into();
        s.head_range_x = 0.;
        s.head_range_y = 0.;
        // Authored head angles are used only when tracking is disabled.
        s.custom_pose.head = [70., 120., -80.];
        let forward = Vec3::new(30., 23., 70.).normalize();
        let up = forward.cross(Vec3::Y.cross(forward).normalize());
        for global in [
            [0., 0., 0.],
            [30., -75., 45.],
            [90., 90., 90.],
            [180., -180., 180.],
        ] {
            for body in [[0., 0., 0.], [45., 80., -50.]] {
                s.model_rotation = global;
                s.custom_pose.body = body;
                let mut p = Pose::new(&s);
                tick(&mut p, &s, &skin, 1., false);
                let (f, u) = head_axes(&s, &p, &skin);
                assert!(
                    f.abs_diff_eq(forward, 0.00001),
                    "{global:?} {body:?}: {f:?}"
                );
                assert!(u.abs_diff_eq(up, 0.00001));
            }
        }
        s.track_mouse = false;
        let mut p = Pose::new(&s);
        tick(&mut p, &s, &skin, 2., false);
        assert!(!head_axes(&s, &p, &skin).0.abs_diff_eq(forward, 0.01));
    }
    #[test]
    fn rotated_head_tracks_screen_directions_and_respects_ranges() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut s = Settings::default();
        s.model_rotation = [55., 100., -65.];
        s.head_range_x = 15.;
        s.head_range_y = 25.;
        let mut p = Pose::new(&s);
        let forward = Vec3::new(30., 23., 70.).normalize();
        let right = Vec3::Y.cross(forward).normalize();
        let up = forward.cross(right);
        for (cursor, sx, sy) in [
            (Vec2::new(100000., 100000.), 1., -1.),
            (Vec2::new(-100000., -100000.), -1., 1.),
        ] {
            p.update(&s, &skin, cursor, 360., 420., false, 1., 1. / 60.);
            let f = head_axes(&s, &p, &skin).0;
            assert!(f.dot(right) * sx > 0.);
            assert!(f.dot(up) * sy > 0.);
            let yaw = f.dot(right).atan2(f.dot(forward)).abs().to_degrees();
            let pitch = (-f.dot(up)).asin().abs().to_degrees();
            assert!((yaw - 15.).abs() < 0.001);
            assert!((pitch - 25.).abs() < 0.001);
        }
    }
    #[test]
    fn custom_typing_target_changes_do_not_teleport_the_arm() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for (side, first, next) in [(1, 81, 84), (0, 80, 89)] {
            let mut s = Settings::default();
            s.posture = "custom".into();
            s.custom_base = "sitting".into();
            s.track_mouse = false;
            s.typing_squash = 0.;
            s.custom_sitting_pose.left_arm = [-30., -20., 25.];
            s.custom_sitting_pose.right_arm = [-40., 30., -35.];
            let mut p = Pose::new(&s);
            for i in 0..120 {
                tick(&mut p, &s, &skin, i as f32 / 120., false);
            }
            let idle = p.arms[side];
            p.key(first, true, 1., &s);
            for i in 0..120 {
                tick(&mut p, &s, &skin, 1. + i as f32 / 120., false);
            }
            let held = p.arms[side];
            assert!(held.angle_between(idle) > 0.1);
            p.key(next, true, 2., &s);
            p.update(&s, &skin, Vec2::ZERO, 360., 420., false, 2., 0.);
            assert!(
                p.arms[side].abs_diff_eq(held, 0.00001),
                "new key moved arm without animation time"
            );
            for i in 0..120 {
                tick(&mut p, &s, &skin, 2. + i as f32 / 120., false);
            }
            let before_release = p.arms[side];
            p.key(next, false, 3., &s);
            p.update(&s, &skin, Vec2::ZERO, 360., 420., false, 3., 0.);
            assert!(
                p.arms[side].abs_diff_eq(before_release, 0.00001),
                "release to another held key teleported arm"
            );
            for i in 0..120 {
                tick(&mut p, &s, &skin, 3. + i as f32 / 120., false);
            }
            let before_idle = p.arms[side];
            p.key(first, false, 4., &s);
            p.update(&s, &skin, Vec2::ZERO, 360., 420., false, 4., 0.);
            assert!(
                p.arms[side].abs_diff_eq(before_idle, 0.00001),
                "release to idle teleported arm"
            );
            tick(&mut p, &s, &skin, 4.01, false);
            assert!(p.arms[side].angle_between(before_idle) < before_idle.angle_between(idle));
            assert!(p.arms[side].angle_between(idle) > 0.01);
            for i in 2..120 {
                tick(&mut p, &s, &skin, 4. + i as f32 / 120., false);
            }
            assert!(p.arms[side].angle_between(idle) < 0.001);
        }
    }
    #[test]
    fn typing_returns_smoothly_after_taps_represses_and_input_reset() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for posture in ["sitting", "custom"] {
            for mode in ["precise", "alternating"] {
                for fps in [30., 60., 240.] {
                    let mut s = Settings::default();
                    s.posture = posture.into();
                    s.custom_base = "sitting".into();
                    s.typing_mode = mode.into();
                    s.track_mouse = false;
                    s.typing_squash = 30.;
                    s.custom_sitting_pose.left_arm = [-70., -45., 60.];
                    s.custom_sitting_pose.right_arm = [25., 55., -80.];
                    let mut p = Pose::new(&s);
                    let dt = 1. / fps;
                    let mut now = 0.;
                    let step = |p: &mut Pose, now: f32, dt: f32| {
                        p.update(&s, &skin, Vec2::ZERO, 360., 420., false, now, dt);
                    };
                    for _ in 0..fps as usize {
                        now += dt;
                        step(&mut p, now, dt);
                    }
                    let idle = p.arms;
                    for vk in [81, 80, 84, 89, 81, 80] {
                        // A tap survives until its minimum press time, then moves
                        // home without changing orientation at zero elapsed time.
                        p.key(vk, true, now, &s);
                        for _ in 0..3 {
                            now += dt;
                            step(&mut p, now, dt);
                        }
                        p.key(vk, false, now, &s);
                        for _ in 0..(fps * 0.22) as usize {
                            now += dt;
                            let before = p.arms;
                            step(&mut p, now, 0.);
                            for side in 0..2 {
                                assert!(p.arms[side].abs_diff_eq(before[side], 0.00001), "pose={posture} mode={mode} fps={fps}: key or timeout teleported arm");
                            }
                            step(&mut p, now, dt);
                        }
                        // New input during the return changes its destination,
                        // preserving the in-flight arm and the squash restart.
                        let before = p.arms;
                        p.key(vk, true, now, &s);
                        assert_eq!(p.squash, 1.);
                        step(&mut p, now, 0.);
                        for side in 0..2 {
                            assert!(p.arms[side].abs_diff_eq(before[side], 0.00001));
                        }
                        now += dt;
                        step(&mut p, now, dt);
                        p.key(vk, false, now, &s);
                    }
                    // Clearing input (such as disabling keyboard interaction)
                    // must not reset the physical arm pose before it can animate.
                    p.key(81, true, now, &s);
                    p.key(80, true, now, &s);
                    for _ in 0..(fps * 0.4) as usize {
                        now += dt;
                        step(&mut p, now, dt);
                    }
                    let before = p.arms;
                    p.reset_keys();
                    step(&mut p, now, 0.);
                    for side in 0..2 {
                        assert!(p.arms[side].abs_diff_eq(before[side], 0.00001));
                    }
                    for _ in 0..fps as usize {
                        now += dt;
                        step(&mut p, now, dt);
                    }
                    for side in 0..2 {
                        assert!(p.arms[side].angle_between(idle[side]) < 0.001, "pose={posture} mode={mode} fps={fps} side={side}: {:?} -> {:?}; error={}", idle[side], p.arms[side], p.arms[side].angle_between(idle[side]));
                    }
                }
            }
        }
    }
    #[test]
    fn custom_seated_idle_typing_swing_and_release() {
        let mut s = Settings::default();
        s.posture = "custom".into();
        s.custom_base = "sitting".into();
        s.track_mouse = false;
        s.custom_sitting_pose.body = [10., 15., 8.];
        s.custom_sitting_pose.right_arm = [-40., 30., -35.];
        s.custom_sitting_pose.left_arm = [-30., -20., 25.];
        s.custom_sitting_pose.left_leg = [15., 10., 5.];
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut p = Pose::new(&s);
        tick(&mut p, &s, &skin, 1., false);
        assert_eq!(p.sit, 1.);
        assert!(parts(&s, &skin, &p).iter().any(|b| b.keyboard));
        let idle = p.arms;
        for (side, vk, left) in [(1, 81, false), (0, 80, true)] {
            p.key(vk, true, 2., &s);
            tick(&mut p, &s, &skin, 2.08, false);
            assert!((p.squash - 0.95).abs() < 0.001);
            for i in 0..120 {
                tick(&mut p, &s, &skin, 2.1 + i as f32 / 120., false);
            }
            let held = p.arms[side];
            assert!(held.angle_between(idle[side]) > 0.05);
            assert_eq!(p.squash, 1.);
            p.click(3.2, left);
            tick(&mut p, &s, &skin, 3.3, false);
            assert!(p.arms[side].angle_between(held) > 0.1);
            tick(&mut p, &s, &skin, 3.6, false);
            assert!(p.arms[side].angle_between(held) < 0.001);
            p.key(vk, false, 3.7, &s);
            for i in 0..120 {
                tick(&mut p, &s, &skin, 3.7 + i as f32 / 120., false);
            }
            assert!(p.arms[side].angle_between(idle[side]) < 0.001);
        }
    }
    #[test]
    fn global_angles_rotate_character_and_keyboard_together() {
        let (mut s, skin, p) = seated();
        let before = parts(&s, &skin, &p);
        s.model_rotation = [35., -75., 22.];
        let after = parts(&s, &skin, &p);
        for a in &before {
            let b = after
                .iter()
                .find(|b| b.uv == a.uv && b.inflate == a.inflate && b.keyboard == a.keyboard)
                .unwrap();
            assert!(b
                .transform
                .abs_diff_eq(model_transform(&s) * a.transform, 0.00001));
            assert_eq!(a.size, b.size);
        }
        assert!(after.iter().any(|b| b.keyboard));
    }
    #[test]
    fn left_swing_mirrors_right_swing() {
        let s = Settings::default();
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut right = Pose::new(&s);
        let mut left = Pose::new(&s);
        right.click(1., false);
        left.click(1., true);
        let mirror = Mat4::from_scale(Vec3::new(-1., 1., 1.));
        for i in 0..37 {
            let t = 1. + i as f32 / 120.;
            tick(&mut right, &s, &skin, t, false);
            tick(&mut left, &s, &skin, t, false);
            assert!((right.body_yaw + left.body_yaw).abs() < 0.00001);
            assert!(Mat4::from_quat(left.arms[0])
                .abs_diff_eq(mirror * Mat4::from_quat(right.arms[1]) * mirror, 0.0001));
        }
    }
    #[test]
    fn left_swing_returns_to_held_or_released_keyboard_hand() {
        let (mut s, skin, mut p) = seated();
        s.main_hand = "left".into();
        let rest = p.arms[0];
        p.key(80, true, 1., &s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, 1. + i as f32 / 120., false);
        }
        let held = p.arms[0];
        let other = p.arms[1];
        p.click(2., true);
        tick(&mut p, &s, &skin, 2.10, false);
        assert!(p.arms[0].angle_between(held) > 0.1);
        assert!(p.arms[1].angle_between(other) < 0.001);
        tick(&mut p, &s, &skin, 2.5, false);
        assert!(p.arms[0].angle_between(held) < 0.001);
        p.click(3., true);
        p.key(80, false, 3.05, &s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, 3. + i as f32 / 120., false);
        }
        assert!(p.arms[0].angle_between(rest) < 0.001);
    }
    #[test]
    fn custom_pose_stays_rigid_and_swing_restores_edited_pose() {
        let mut s = Settings::default();
        s.posture = "custom".into();
        s.track_mouse = false;
        s.custom_pose.head = [15., 20., -5.];
        s.custom_pose.body = [20., -10., 15.];
        s.custom_pose.left_arm = [-70., 25., 50.];
        s.custom_pose.right_arm = [-30., 15., -20.];
        s.custom_pose.left_leg = [25., 15., 10.];
        s.custom_pose.right_leg = [-25., -15., -10.];
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut p = Pose::new(&s);
        tick(&mut p, &s, &skin, 1., true);
        assert_eq!(p.crouch, 0.);
        assert_eq!(p.sit, 0.);
        let boxes = parts(&s, &skin, &p);
        assert_eq!(boxes.len(), 12);
        assert!(!boxes.iter().any(|b| b.keyboard));
        for b in &boxes {
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!((b.transform.transform_vector3(axis).length() - 1.).abs() < 0.0001);
            }
        }
        p.click(2., true);
        tick(&mut p, &s, &skin, 2.1, false);
        s.custom_pose.left_arm = [-110., -30., 60.];
        tick(&mut p, &s, &skin, 2.5, false);
        assert!(p.arms[0].angle_between(degrees(&s.custom_pose.left_arm)) < 0.001);
        p.key(81, true, 3., &s);
        assert!(p.held_codes().is_empty());
    }
    #[test]
    fn translucent_face_has_no_diagonal_crack_or_double_blend() {
        let atlas = RgbaImage::from_pixel(64, 64, image::Rgba([100, 150, 200, 128]));
        let mut png = std::io::Cursor::new(Vec::new());
        atlas.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let skin = Skin::load(png.get_ref()).unwrap();
        let b = BoxPart {
            size: Vec3::splat(8.),
            uv: Vec2::new(32., 0.),
            transform: Mat4::IDENTITY,
            inflate: 1.,
            mirror: false,
            keyboard: false,
            cape: false,
            faces: None,
            eyes: None,
            eyelid: 1.,
            pupil: Vec2::ZERO,
        };
        let points = [
            Vec3::new(2.5, 2.5, 1.),
            Vec3::new(13.5, 2.5, 1.),
            Vec3::new(2.5, 13.5, 1.),
            Vec3::new(13.5, 13.5, 1.),
        ];
        for triangles in [
            [[0, 1, 2], [1, 3, 2]],
            [[2, 1, 0], [2, 3, 1]],
            [[0, 1, 3], [0, 3, 2]],
        ] {
            let mut frame = Frame::new(16, 16);
            for ids in triangles {
                frame.triangle(
                    ids.map(|i| points[i]),
                    [Vec2::new(40., 8.); 3],
                    4,
                    &b,
                    Tex::of(&skin.image, &skin.colors),
                );
            }
            for y in 3..13 {
                for x in 3..13 {
                    assert_eq!(frame.pixels[(y * 16 + x) * 4 + 3], 128, "({x},{y})");
                }
            }
        }
    }
    #[test]
    fn keyboard_custom_color_is_rendered() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut s = Settings::default(); s.posture = "sitting".into(); s.keyboard_color = "#FF0000".into();
        let p = Pose::new(&s); let mut f = Frame::new(360, 420);
        f.draw(&s, &skin, &p);
        assert!(f.pixels.chunks_exact(4).filter(|p| p[0] == 0 && p[1] == 0 && p[2] > 200 && p[3] > 0).count() > 100);
    }
    #[test]
    fn chibi_typing_hands_reach_scaled_keyboard_surface() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for scale in [50., 100., 180.] { for squash in [1., 0.95, 0.7] {
            let mut s = Settings::default(); s.chibi = true; s.chibi_body_scale = scale;
            s.posture = "sitting".into(); s.outer_layer = false; s.breathing_animation = false;
            s.model_rotation = [0.; 3];
            let mut p = Pose::new(&s); p.sit = 1.; p.squash = squash;
            for key in [65, 81, 90, 80, 13, 32, 112, 123] {
                let pos = key_position(key).unwrap();
                let side = if pos.x >= 0. { 0 } else { 1 };
                p.arms[side] = chibi_typing_rotation(&s, &p, side, Some(pos), true);
                let b = parts(&s, &skin, &p);
                let board = b.iter().find(|b| b.keyboard).unwrap();
                let wrist = b[2 + side].transform.transform_point3(Vec3::new(0., -2., 0.));
                let local = board.transform.inverse().transform_point3(wrist);
                assert!((local.y - 0.2).abs() < 1e-4, "key {key}, scale {scale}: {local:?}");
                assert!(local.x.abs() < 4.5 && local.z.abs() < 1.2, "outside board: {local:?}");
            }
        }}
    }
    #[test]
    fn chibi_eyelids_cover_correct_face_pixels_at_all_body_scales() {
        let skin = blinkable_skin();
        for scale in [50., 100., 180.] {
            let mut s = Settings::default(); s.chibi = true; s.da_blink = true; s.chibi_body_scale = scale;
            let p = Pose::new(&s);
            let mut head = parts(&s, &skin, &p).into_iter().find(|b| b.eyes.is_some()).unwrap();
            head.transform = Mat4::from_translation(Vec3::Y * 15.)
                * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Z, Vec3::new(30., 23., 70.).normalize()));
            head.eyelid = 0.;
            let mut f = Frame::new(450, 525);
            f.draw_box(&head, Tex::of(&skin.image, &skin.colors));
            for eye in [head.eyes.unwrap().left, head.eyes.unwrap().right] {
                let point = Vec3::new(((eye.x0 as f32 + eye.x1 as f32 + 1.)/2.-4.) * 0.9,
                    (4.-eye.y0 as f32-eye.rows as f32/2.)*0.9, 3.62);
                let screen = project(head.transform.transform_point3(point), 450., 525.);
                let offset = (screen.y as usize * 450 + screen.x as usize)*4;
                let expected = skin.colors[(eye.lid.1 as u32*skin.image.width()+eye.lid.0 as u32) as usize][4];
                assert_eq!([f.pixels[offset+2],f.pixels[offset+1],f.pixels[offset]], expected);
            }
        }
    }
    #[test]
    fn chibi_hat_top_matches_skin_atlas_front_back_and_left_right() {
        for factor in [1u32, 2] {
            let mut atlas = RgbaImage::new(64 * factor, 64 * factor);
            for (x, y, pixel) in atlas.enumerate_pixels_mut() {
                *pixel = image::Rgba([(x / factor * 3 + 10) as u8, (y / factor * 3 + 10) as u8, 100, 255]);
            }
            let mut png = std::io::Cursor::new(Vec::new());
            atlas.write_to(&mut png, image::ImageFormat::Png).unwrap();
            let skin = Skin::load(png.get_ref()).unwrap();
            for scale in [50., 100., 180.] {
                let mut s = Settings::default();
                s.chibi = true; s.outer_layer = true; s.chibi_body_scale = scale;
                let p = Pose::new(&s);
                let mut hat = parts(&s, &skin, &p).into_iter().find(|b| {
                    b.inflate > 0. && b.faces.is_some_and(|f| f[2].u0 == 10.)
                }).expect("Q版头部外层");
                let orientation = Quat::from_rotation_arc(Vec3::Y, Vec3::new(30., 23., 70.).normalize());
                hat.transform = Mat4::from_translation(Vec3::Y * 15.) * Mat4::from_quat(orientation);
                let size = hat.size + Vec3::splat(hat.inflate);
                let mut frame = Frame::new(450, 525);
                frame.draw_box(&hat, Tex::of(&skin.image, &skin.colors));
                // Anatomical coordinates: +Z is front, +X is character-left.
                // The hat top occupies atlas x=40..48, y=0..8, just like the
                // regular model. Four different corner colors catch both flips.
                for x in [0.5f32, 7.5] { for z in [0.5f32, 7.5] {
                    let local = Vec3::new((x / 8. - 0.5) * size.x, size.y / 2., (z / 8. - 0.5) * size.z);
                    let screen = project(hat.transform.transform_point3(local), 450., 525.);
                    let offset = (screen.y as usize * 450 + screen.x as usize) * 4;
                    let texel = (z * factor as f32) as u32 * skin.image.width() + ((40. + x) * factor as f32) as u32;
                    let expected = skin.colors[texel as usize][2];
                    assert_eq!([frame.pixels[offset+2], frame.pixels[offset+1], frame.pixels[offset]], expected,
                        "hat top: factor={factor}, scale={scale}, corner=({x},{z})");
                }}
            }
        }
    }
    #[test]
    fn unfolded_skin_faces_and_corner_directions() {
        // Coordinate-coded PNG is independent of the former Electron renderer:
        // every texel identifies its own atlas X/Y, so opposite ends cannot alias.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.tmp/uv-validation");
        std::fs::create_dir_all(&dir).unwrap();
        let cases = [
            ("head", 8., 8., 8., 0., 0.),
            ("hat", 8., 8., 8., 32., 0.),
            ("body", 8., 12., 4., 16., 16.),
            ("coat", 8., 12., 4., 16., 32.),
            ("right-arm", 4., 12., 4., 40., 16.),
            ("left-arm", 4., 12., 4., 32., 48.),
            ("right-sleeve", 4., 12., 4., 40., 32.),
            ("left-sleeve", 4., 12., 4., 48., 48.),
            ("right-leg", 4., 12., 4., 0., 16.),
            ("left-leg", 4., 12., 4., 16., 48.),
            ("right-trouser", 4., 12., 4., 0., 32.),
            ("left-trouser", 4., 12., 4., 0., 48.),
            ("slim-right", 3., 12., 4., 40., 16.),
            ("slim-left", 3., 12., 4., 32., 48.),
            ("slim-sleeve-right", 3., 12., 4., 40., 32.),
            ("slim-sleeve-left", 3., 12., 4., 48., 48.),
        ];
        for factor in [1u32, 2] {
            let mut atlas = RgbaImage::new(64 * factor, 64 * factor);
            for (x, y, p) in atlas.enumerate_pixels_mut() {
                *p = image::Rgba([
                    (x / factor * 3 + 10) as u8,
                    (y / factor * 3 + 10) as u8,
                    100,
                    255,
                ]);
            }
            let mut png = std::io::Cursor::new(Vec::new());
            atlas.write_to(&mut png, image::ImageFormat::Png).unwrap();
            let skin = Skin::load(png.get_ref()).unwrap();
            for &(name, w, h, d, u, v) in &cases {
                for mirror in [false, true] {
                    for face in 0..6 {
                        let normal =
                            [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z][face];
                        let n = if mirror {
                            Vec3::new(-normal.x, normal.y, normal.z)
                        } else {
                            normal
                        };
                        let orientation =
                            Quat::from_rotation_arc(n, Vec3::new(30., 23., 70.).normalize());
                        let transform =
                            Mat4::from_translation(Vec3::Y * 15.) * Mat4::from_quat(orientation);
                        let part = BoxPart {
                            size: Vec3::new(w, h, d),
                            uv: Vec2::new(u, v),
                            transform,
                            inflate: 0.,
                            mirror,
                            keyboard: false,
                            cape: false,
                            faces: None,
                            eyes: None,
                            eyelid: 1.,
                            pupil: Vec2::ZERO,
                        };
                        let mut frame = Frame::new(450, 525);
                        frame.draw_box(&part, Tex::of(&skin.image, &skin.colors));
                        for a in [0.5, if face < 2 { d - 0.5 } else { w - 0.5 }] {
                            for b in [
                                0.5,
                                if face == 2 || face == 3 {
                                    d - 0.5
                                } else {
                                    h - 0.5
                                },
                            ] {
                                // Explicit cube-net coordinates and anatomical face positions.
                                let (point, tx, ty) = match face {
                                    0 => (
                                        Vec3::new(w / 2., h / 2. - b, d / 2. - a),
                                        u + d + w + a,
                                        v + d + b,
                                    ),
                                    1 => (
                                        Vec3::new(-w / 2., h / 2. - b, a - d / 2.),
                                        u + a,
                                        v + d + b,
                                    ),
                                    2 => (
                                        Vec3::new(a - w / 2., h / 2., b - d / 2.),
                                        u + d + a,
                                        v + b,
                                    ),
                                    3 => (
                                        Vec3::new(a - w / 2., -h / 2., b - d / 2.),
                                        u + d + w + a,
                                        v + b,
                                    ),
                                    4 => (
                                        Vec3::new(a - w / 2., h / 2. - b, d / 2.),
                                        u + d + a,
                                        v + d + b,
                                    ),
                                    _ => (
                                        Vec3::new(w / 2. - a, h / 2. - b, -d / 2.),
                                        u + 2. * d + w + a,
                                        v + d + b,
                                    ),
                                };
                                let point = if mirror {
                                    Vec3::new(-point.x, point.y, point.z)
                                } else {
                                    point
                                };
                                let screen = project(transform.transform_point3(point), 450., 525.);
                                let i = (screen.y as usize * 450 + screen.x as usize) * 4;
                                let expected = skin.colors[((ty * factor as f32) as u32
                                    * skin.image.width()
                                    + (tx * factor as f32) as u32)
                                    as usize][face];
                                assert_eq!([frame.pixels[i+2],frame.pixels[i+1],frame.pixels[i]],expected,"{name} factor={factor} mirror={mirror} face={face} sample=({a},{b})");
                            }
                        }
                        if factor == 1 && !mirror && name == "head" {
                            image::save_buffer(
                                dir.join(format!("head-face-{face}.png")),
                                &frame.rgba(),
                                450,
                                525,
                                image::ColorType::Rgba8,
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
    fn tick(p: &mut Pose, s: &Settings, skin: &Skin, t: f32, shift: bool) {
        p.update(
            s,
            skin,
            Vec2::new(180., 160.),
            360.,
            420.,
            shift,
            t,
            1. / 120.,
        );
    }
    fn seated() -> (Settings, Skin, Pose) {
        let mut s = Settings::default();
        s.posture = "sitting".into();
        s.track_mouse = false;
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut p = Pose::new(&s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, i as f32 / 120., false);
        }
        (s, skin, p)
    }
    fn seated_chibi() -> (Settings, Skin, Pose) {
        let mut s = Settings::default();
        s.posture = "sitting".into();
        s.chibi = true;
        s.track_mouse = false;
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut p = Pose::new(&s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, i as f32 / 120., false);
        }
        (s, skin, p)
    }
    /// 坐姿 Q 版：腿保持身体下方，不会翻到头上方；身体因坐下而下降。
    #[test]
    fn chibi_seated_legs_stay_below_body() {
        let (s, skin, p) = seated_chibi();
        let parts = parts_with_cape(&s, &skin, &p, None);
        let center = |p: &BoxPart| p.transform.transform_point3(Vec3::ZERO);
        let body = parts
            .iter()
            .find(|p| (p.size.y - 3.7).abs() < 0.01 && (p.size.x - 3.5).abs() < 0.01)
            .expect("身体");
        let bc = center(body);
        // 坐姿身体中心 chibi 缩放后约 5.5（站立约 10）。
        assert!(bc.y < 8.0, "坐姿身体应下降，实际 {}", bc.y);
        for l in parts.iter().filter(|p| {
            (p.size.x - 1.5).abs() < 0.01
                && (p.size.y - 4.8).abs() < 0.01
                && (p.size.z - 1.5).abs() < 0.01
                && center(p).y < bc.y
        }) {
            assert!(
                center(l).y < bc.y,
                "坐姿腿不应翻到身体上方，腿 {:?} 身体 {:?}",
                center(l),
                bc
            );
        }
    }
    /// Q 版模式：生成 12 个 cuboid（头2+身2+臂4+腿4，无图腾），每个都带
    /// per-face UV，渲染后一帧有可见像素。
    #[test]
    fn chibi_mode_renders_without_totem() {
        let mut s = Settings::default();
        s.chibi = true;
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let pose = Pose::new(&s);
        let parts = parts_with_cape(&s, &skin, &pose, None);
        assert_eq!(parts.len(), 12, "头2+身2+臂4+腿4=12，图腾已被去掉");
        assert!(
            parts.iter().all(|p| p.faces.is_some()),
            "Q 版每个面都走独立 UV"
        );
        let mut frame = Frame::new(450, 525);
        frame.draw_ext(&s, &skin, None, &pose);
        assert!(
            frame.bounds[2] > frame.bounds[0] && frame.bounds[3] > frame.bounds[1],
            "Q 版渲染应有可见像素"
        );
    }
    /// Q 版兼容全部动作：站、坐、潜行、打字、挥手逐一遍历，渲染不崩溃且每帧有像素。
    #[test]
    fn chibi_renders_all_postures_without_panic() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for (posture, shift, do_key) in [
            ("standing", false, false),
            ("sitting", false, false),
            ("standing", true, false), // 潜行
            ("sitting", false, true),  // 打字
        ] {
            let mut s = Settings::default();
            s.chibi = true;
            s.posture = posture.into();
            s.track_mouse = false;
            let mut p = Pose::new(&s);
            for i in 0..120 {
                if do_key && i == 60 {
                    p.key(81, true, 0.5, &s);
                }
                tick(&mut p, &s, &skin, i as f32 / 120., shift);
            }
            // 挥手一次，确认挥动也不破坏渲染。
            p.click(1., false);
            for i in 0..30 {
                tick(&mut p, &s, &skin, 1. + i as f32 / 120., false);
            }
            let mut frame = Frame::new(450, 525);
            frame.draw_ext(&s, &skin, None, &p);
            assert!(
                frame.bounds[2] > frame.bounds[0] && frame.bounds[3] > frame.bounds[1],
                "姿势 {posture}（shift={shift} key={do_key}）渲染应有可见像素"
            );
        }
    }
    /// Q 版几何：头在身体上方、腿在身体下方、左右手臂/腿关于 x 轴对称、
    /// 头居中。用 size 特征识别部位，不依赖 push 顺序。
    #[test]
    fn chibi_geometry_is_well_formed() {
        let mut s = Settings::default();
        s.chibi = true;
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let pose = Pose::new(&s);
        let parts = parts_with_cape(&s, &skin, &pose, None);
        let center = |p: &BoxPart| p.transform.transform_point3(Vec3::ZERO);
        let head = parts.iter().find(|p| (p.size.y - 7.2).abs() < 0.01).expect("头部");
        let body = parts
            .iter()
            .find(|p| (p.size.y - 3.7).abs() < 0.01 && (p.size.x - 3.5).abs() < 0.01)
            .expect("身体");
        let hc = center(head);
        let bc = center(body);
        // 手臂 inner 1.2×4×1.2，腿 inner 1.5×4.8×1.5，用宽区分。
        let arms: Vec<_> = parts
            .iter()
            .filter(|p| {
                (p.size.x - 1.2).abs() < 0.01
                    && (p.size.y - 4.0).abs() < 0.01
                    && (p.size.z - 1.2).abs() < 0.01
            })
            .collect();
        let legs: Vec<_> = parts
            .iter()
            .filter(|p| {
                (p.size.x - 1.5).abs() < 0.01
                    && (p.size.y - 4.8).abs() < 0.01
                    && (p.size.z - 1.5).abs() < 0.01
            })
            .collect();
        assert_eq!(arms.len(), 2, "应有左右两条手臂");
        assert_eq!(legs.len(), 2, "应有左右两条腿");
        assert!(hc.y > bc.y, "头应在身体上方");
        for l in &legs {
            assert!(center(l).y < bc.y, "腿应在身体下方");
        }
        assert!((hc.x).abs() < 0.01, "头应居中");
        let mut ax = arms.iter().map(|a| center(a).x);
        let (a0, a1) = (ax.next().unwrap(), ax.next().unwrap());
        assert!((a0 + a1).abs() < 0.01, "左右手臂应关于 x 轴对称");
        let mut lx = legs.iter().map(|l| center(l).x);
        let (l0, l1) = (lx.next().unwrap(), lx.next().unwrap());
        assert!((l0 + l1).abs() < 0.01, "左右腿应关于 x 轴对称");
    }
    /// 潜行扭动：材质包先累计 `var.SneakingTimer` 才启动 `SneakyDanceTimer`，
    /// 依照所提供资源包约 3.33 秒开始，松开后回落。
    #[test]
    fn da_blink_is_independent_of_gaze_and_available_for_chibi() {
        let mut s = Settings::default();
        let skin = blinkable_skin();
        let mut p = Pose::new(&s);
        for now in [0., 1., 2., 3., 4., 5.] {
            p.update(&s, &skin, Vec2::new(180., -5000.), 360., 420., false, now, 0.);
            let opening = p.eyelid;
            p.update(&s, &skin, Vec2::new(180., 5000.), 360., 420., false, now, 0.);
            assert_eq!(p.eyelid, opening);
            assert_eq!(p.pupil, Vec2::ZERO);
        }
        s.da_blink = false;
        p.update(&s, &skin, Vec2::ZERO, 360., 420., false, 1., 0.);
        assert_eq!(p.eyelid, 1.);
        assert!(parts(&s, &skin, &p).iter().all(|b| b.eyes.is_none()));
        s.da_blink = true; s.chibi = true;
        p.update(&s, &skin, Vec2::ZERO, 360., 420., false, 1., 0.);
        assert_eq!(p.eyelid, 1. - p.blink_at(1.));
        assert_eq!(parts(&s, &skin, &p).iter().filter(|b| b.eyes.is_some()).count(), 1);
    }
    #[test]
    fn dance_starts_immediately_and_auto_crouches_only_when_standing() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for posture in ["standing", "sitting"] { for chibi in [false,true] {
            let mut s=Settings::default();s.posture=posture.into();s.chibi=chibi;s.da_dance=true;
            let mut p=Pose::new(&s);
            p.update(&s,&skin,Vec2::ZERO,360.,420.,false,0.01,0.01);
            if posture=="standing" {assert_eq!(p.dance,1.);assert_eq!(p.crouch,1.);assert!(p.body_roll.abs()>0.001);}
            else {assert_eq!(p.dance,0.);assert_eq!(p.crouch,0.);}
            assert!((p.leg_roll+p.body_roll).abs()<1e-6);
            s.da_dance=false;
            p.update(&s,&skin,Vec2::ZERO,360.,420.,false,0.02,0.01);
            assert_eq!(p.dance,0.);assert_eq!(p.body_roll,0.);
        }}
    }
    #[test]
    #[ignore = "writes a local visual review sheet when CHIBI_REVIEW_PATH is set"]
    fn chibi_dance_review_sheet() {
        let output = std::env::var("CHIBI_REVIEW_PATH").expect("CHIBI_REVIEW_PATH");
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut sheet = image::RgbaImage::from_pixel(1440, 1260, image::Rgba([225, 229, 235, 255]));
        for (row, scale) in [50., 100., 180.].iter().enumerate() {
            let mut s = Settings::default();
            s.chibi = true; s.chibi_body_scale = *scale; s.da_dance = true;
            s.track_mouse = false; s.outer_layer = true; s.breathing_animation = false;
            let mut p = Pose::new(&s);
            for frame in 0..120 {
                p.update(&s, &skin, Vec2::ZERO, 360., 420., false, frame as f32 / 60., 1. / 60.);
                if frame % 30 == 15 {
                    let mut f = Frame::new(360, 420);
                    f.draw(&s, &skin, &p);
                    let tile = image::RgbaImage::from_raw(360, 420, f.rgba()).unwrap();
                    image::imageops::overlay(&mut sheet, &tile, (frame / 30 * 360) as i64, (row * 420) as i64);
                }
            }
        }
        sheet.save(output).unwrap();
    }
    #[test]
    fn chibi_cape_tracks_torso_and_body_slider() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let cape = Cape { image: image::RgbaImage::new(64, 32), colors: vec![] };
        for posture in ["standing", "sitting", "custom"] {
            for scale in [50., 100., 180.] {
                let mut s = Settings::default();
                s.chibi = true; s.chibi_body_scale = scale; s.posture = posture.into();
                s.da_dance = true; s.outer_layer = false;
                let mut p = Pose::new(&s);
                p.update(&s, &skin, Vec2::ZERO, 360., 420., false, 1.2, 1./60.);
                let without = parts_with_cape(&s, &skin, &p, None);
                let with = parts_with_cape(&s, &skin, &p, Some(&cape));
                assert_eq!(with.len(), without.len() + 1);
                let c = with.iter().find(|p| p.cape).unwrap();
                let torso = &with[1];
                let local = torso.transform.inverse() * c.transform;
                let expected = Mat4::from_translation(Vec3::new(0., 3.6 * (3.7 / 12.) - 1.85, -2.9 * (2.5 / 4.)))
                    * Mat4::from_scale(Vec3::new(3.5 / 8., 3.7 / 12., 2.5 / 4.))
                    * Mat4::from_rotation_x(0.14);
                assert!(local.abs_diff_eq(expected, 0.0001));
                assert_eq!(c.size, Vec3::new(10., 16., 1.));
            }
        }
    }
    #[test]
    #[ignore = "local DLmember visual review"]
    fn dlmember_chibi_cape_review() {
        let dir = std::path::PathBuf::from(std::env::var("CAPE_REVIEW_DIR").unwrap());
        let skin = Skin::load(&std::fs::read(dir.join("skin.png")).unwrap()).unwrap();
        let cape = Cape::load(&std::fs::read(dir.join("cape.png")).unwrap()).unwrap();
        let mut sheet = image::RgbaImage::new(1440, 1260);
        for (row, scale) in [50.,100.,180.].into_iter().enumerate() {
            for (col, (posture, dance, yaw)) in [("standing",false,150.),("sitting",false,150.),("standing",true,150.),("standing",false,0.)].into_iter().enumerate() {
                let mut s = Settings::default();
                s.chibi = true; s.chibi_body_scale = scale; s.posture = posture.into();
                s.arm_model = "slim".into(); s.show_cape = true; s.da_dance = dance; s.model_rotation = [0.,yaw,0.]; s.track_mouse = false;
                let mut p = Pose::new(&s);
                for i in 0..120 { p.update(&s,&skin,Vec2::ZERO,360.,420.,false,i as f32/60.,1./60.); }
                let mut f = Frame::new(360,420); f.draw_ext(&s,&skin,Some(&cape),&p);
                let shown = f.rgba();
                s.show_cape = false; f.draw_ext(&s,&skin,Some(&cape),&p);
                if col < 3 { assert!(shown != f.rgba(), "cape not visible: row={row}, col={col}"); }
                let tile = image::RgbaImage::from_raw(360,420,shown).unwrap();
                image::imageops::overlay(&mut sheet,&tile,(col*360) as i64,(row*420) as i64);
            }
        }
        sheet.save(dir.join("review.png")).unwrap();
    }
    #[test]
    fn chibi_dance_joint_pivots_remain_attached_at_all_scales() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for scale in [50., 100., 180.] { for slim in [false, true] {
            let mut s = Settings::default();
            s.chibi = true; s.chibi_body_scale = scale; s.da_dance = true;
            s.outer_layer = false; s.track_mouse = false;
            s.arm_model = if slim { "slim" } else { "classic" }.into();
            let mut p = Pose::new(&s);
            for frame in 0..240 {
                p.update(&s, &skin, Vec2::ZERO, 360., 420., false, frame as f32 / 60., 1. / 60.);
                let b = parts(&s, &skin, &p);
                for side in 0..2 {
                    let sign = if side == 0 { 1. } else { -1. };
                    let hip = b[1].transform.transform_point3(Vec3::new(sign * 0.75, -1.85, 0.));
                    let leg_top = b[4 + side].transform.transform_point3(Vec3::Y * 2.4);
                    assert!(hip.distance(leg_top) < 0.0001, "hip drift: scale={scale}, frame={frame}");
                    let shoulder = b[1].transform.transform_point3(Vec3::new(sign * 2.35, 1.85, 0.));
                    let arm_top = b[2 + side].transform.transform_point3(Vec3::Y * 2.);
                    assert!(shoulder.distance(arm_top) < 0.0001, "shoulder drift: scale={scale}, frame={frame}");
                }
            }
        }}
    }
    #[test]
    fn chibi_dance_amplitude_scales_without_scaling_other_actions() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let sample = |chibi, scale| {
            let mut s = Settings::default();
            s.posture = "standing".into(); s.da_dance = true; s.chibi = chibi; s.chibi_body_scale = scale;
            let mut p = Pose::new(&s);
            p.update(&s, &skin, Vec2::ZERO, 360., 420., false, 0.1, 0.1);
            p
        };
        let normal = sample(false, 100.);
        for scale in [50., 100., 180.] {
            let p = sample(true, scale);
            let factor = 3.7 / 12. * scale / 100.;
            for (actual, source) in [(p.body_roll, normal.body_roll), (p.arm_roll, normal.arm_roll), (p.leg_roll, normal.leg_roll)] {
                assert!((actual - source * factor).abs() < 1e-6);
            }
            assert!((p.body_bob * scale / 100. - normal.body_bob * factor).abs() < 1e-6);
        }
    }
    #[test]
    fn chibi_shells_are_centered_and_scale_with_their_own_body_part() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for scale in [50., 100., 180.] { for slim in [false, true] { for posture in ["standing", "sitting", "custom"] {
            let mut s = Settings::default();
            s.chibi = true; s.chibi_body_scale = scale; s.outer_layer = true;
            s.posture = posture.into(); s.arm_model = if slim { "slim" } else { "classic" }.into();
            let p = Pose::new(&s);
            let b = parts(&s, &skin, &p);
            let originals = [Vec3::splat(8.), Vec3::new(8.,12.,4.), Vec3::new(if slim {3.} else {4.},12.,4.), Vec3::new(if slim {3.} else {4.},12.,4.), Vec3::new(4.,12.,4.), Vec3::new(4.,12.,4.)];
            for (index, original) in originals.iter().enumerate() {
                let inner = &b[index];
                let outer = b.iter().find(|part| part.inflate > 0. && part.transform.abs_diff_eq(inner.transform, 1e-6)).expect("centered matching shell");
                let gap = outer.size + Vec3::splat(outer.inflate) - inner.size;
                let expected = inner.size / *original * if index == 0 {1.} else {0.5};
                assert!(gap.abs_diff_eq(expected, 1e-6));
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    let world_gap = outer.transform.transform_vector3(gap * axis).length();
                    let body_factor = if index == 0 {1.} else {scale/100.};
                    assert!((world_gap - (expected * axis).length() * 1.5 * body_factor).abs() < 1e-5);
                }
            }
        }}}
    }
    #[test]
    fn chibi_body_scales_uniformly_but_head_keeps_dimensions() {
        let mut s=Settings::default();s.chibi=true;s.outer_layer=false;s.da_dance=false;
        let skin=Skin::load(DEFAULT_SKIN).unwrap();let p=Pose::new(&s);
        let a=parts(&s,&skin,&p);s.chibi_body_scale=150.;let b=parts(&s,&skin,&p);
        for axis in [Vec3::X,Vec3::Y,Vec3::Z] {
            assert!((a[0].transform.transform_vector3(axis).length()-b[0].transform.transform_vector3(axis).length()).abs()<1e-6);
            for i in 1..6 {assert!((b[i].transform.transform_vector3(axis).length()/a[i].transform.transform_vector3(axis).length()-1.5).abs()<1e-5);}
        }
    }
    #[test]
    fn crouch_is_immediate() {
        let s = Settings::default();
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut p = Pose::new(&s);
        tick(&mut p, &s, &skin, 1., true);
        assert_eq!(p.crouch, 1.);
        tick(&mut p, &s, &skin, 1.01, false);
        assert_eq!(p.crouch, 0.);
    }
    /// 眨眼曲线必须与资源包一致：闭眼只占很短一段，随后完全睁开，
    /// 不会长时间停在闭眼，也不会出现负值或超过 1。
    #[test]
    fn blink_curve_matches_the_resource_pack() {
        let mut samples = Vec::new();
        for i in 0..(60 * 600) {
            let u = i as f32 / 600. * std::f32::consts::TAU * 3.;
            let c = blink_closure(u);
            assert!((0. ..=1.).contains(&c), "closure {c} out of range at u={u}");
            samples.push(c);
        }
        let closed = samples.iter().filter(|&&c| c > 0.99).count();
        let ratio = closed as f32 / samples.len() as f32;
        assert!(
            (0.01..0.05).contains(&ratio),
            "closed-eye ratio {ratio} should be a short blink, not a stare"
        );
    }
    /// 眨眼间隔来自同一个正弦叠加，因此并不固定；同时每次闭眼都很短。
    #[test]
    fn blink_intervals_vary_and_stay_short() {
        let p = Pose::new(&Settings::default());
        let mut starts = Vec::new();
        let mut open = true;
        let dt = 1. / 240.;
        for i in 0..(240 * 120) {
            let now = i as f32 * dt;
            let lid = 1. - p.blink_at(now);
            if open && lid > 0.5 {
                open = false;
                starts.push(now);
            } else if !open && lid < 0.5 {
                open = true;
            }
        }
        assert!(starts.len() >= 8, "only {} blinks in two minutes", starts.len());
        let gaps: Vec<f32> = starts.windows(2).map(|w| w[1] - w[0]).collect();
        let min = gaps.iter().cloned().fold(f32::INFINITY, f32::min);
        let max = gaps.iter().cloned().fold(0., f32::max);
        assert!(min >= 1.5, "blinks are too frequent: {min}s apart");
        assert!(max - min > 1.0, "blink interval is a fixed metronome: {gaps:?}");
        assert!(gaps.iter().all(|g| *g < 20.), "blinks are too rare: {gaps:?}");
    }
    /// 按 wiki「眼睛画法」造一张可眨眼的皮肤：在原版皮肤上，把眼睛素材画进
    /// 材质包的识别格 (4,6)-(5,7)——上一行是眼皮色、下一行是眼白+瞳孔。
    fn blinkable_skin() -> Skin {
        use image::{Rgba, RgbaImage};
        let base = Skin::load(DEFAULT_SKIN).unwrap().image;
        let mut atlas = base.clone();
        let lid = [0xd7u8, 0xa3, 0x79];
        let white = [0xf2u8, 0xe4, 0xc9];
        let pupil = [0x3bu8, 0x61, 0x4c];
        // 识别格：眼睛行 y=6（眼皮色），瞳孔行 y=7（眼白 + 瞳孔）。
        atlas.put_pixel(4, 6, Rgba([lid[0], lid[1], lid[2], 255]));
        atlas.put_pixel(5, 6, Rgba([lid[0], lid[1], lid[2], 255]));
        atlas.put_pixel(4, 7, Rgba([white[0], white[1], white[2], 255]));
        atlas.put_pixel(5, 7, Rgba([pupil[0], pupil[1], pupil[2], 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        atlas.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let _ = RgbaImage::new(1, 1);
        Skin::load(png.get_ref()).expect("synthetic blinkable skin must load")
    }
    /// 眨眼只影响头部正面的眼皮，不会改变任何骨骼的位置：睁眼与闭眼时，
    /// 身体、四肢、键盘的变换矩阵必须完全一致，不能出现角色跳动。
    #[test]
    fn blink_only_touches_the_face_not_the_body() {
        let s = Settings::default();
        let skin = blinkable_skin();
        assert!(skin.eyes.is_some(), "wiki-modified skin must expose its eyes");
        let mut closed = Pose::new(&s);
        // 找到最近的一次完全闭眼。
        let closed_at = (0..2400)
            .map(|i| i as f32 / 240.)
            .find(|t| closed.blink_at(*t) > 0.99)
            .expect("the curve must reach a fully closed frame");
        // 身体现在跟着视线和时间轻摆，两帧必须取同一时刻才谈得上"只换眼态"；
        // 换个相位让这一刻刚好完全睁开。
        let mut open = Pose::new(&s);
        open.blink_phase = (0..3600)
            .map(|i| i as f32 / 3600. * std::f32::consts::TAU)
            .find(|phase| blink_closure(phase + closed_at * BLINK_RATE) < 0.01)
            .expect("some phase keeps the eyes open at that instant");
        tick(&mut open, &s, &skin, closed_at, false);
        tick(&mut closed, &s, &skin, closed_at, false);
        assert!((1. - closed.eyelid) > 0.99, "{}", closed.eyelid);
        // 睁眼程度还要乘材质包的 `eye_top.ty` 开合，所以"睁开"不一定是 1，
        // 但必须明显大于闭眼那一帧。
        assert!(open.eyelid >= 0.5, "{}", open.eyelid);
        assert!(open.eyelid > closed.eyelid + 0.4, "{}", open.eyelid);
        let a = parts(&s, &skin, &open);
        let b = parts(&s, &skin, &closed);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.size, y.size);
            assert!(x.transform.abs_diff_eq(y.transform, 1e-6));
            assert_eq!(x.eyes.is_some(), y.eyes.is_some());
        }
    }
    /// 材质包的契约：没有识别格的眼睛素材就不眨眼。原版皮肤应当返回 None，
    /// 在识别格里画过眼睛之后才返回 Some。
    #[test]
    fn skin_without_eye_cells_never_blinks() {
        let plain = Skin::load(DEFAULT_SKIN).unwrap();
        assert!(
            plain.eyes.is_none(),
            "an unmodified skin has no eye cells and must not blink"
        );
        assert!(blinkable_skin().eyes.is_some());
    }
    /// 真正画出像素：闭眼时眼睛区域的画面必须与睁眼不同，且颜色来自
    /// 眼睛上方那一行（眼皮色），松开后恢复原样。
    #[test]
    fn closed_eyes_change_the_face_pixels_and_then_restore() {
        let s = Settings::default();
        let skin = blinkable_skin();
        let eyes = skin.eyes.unwrap();
        let mut p = Pose::new(&s);
        let phase = p.blink_phase;
        let mut render = |now: f32| {
            p.update(&s, &skin, Vec2::ZERO, 360., 420., false, now, 0.);
            let mut f = Frame::new(360, 420);
            f.draw(&s, &skin, &p);
            (f.rgba(), p.eyelid)
        };
        let (open, _) = render(0.);
        let t = (0..2400)
            .map(|i| i as f32 / 240.)
            .find(|t| {
                let u = phase + *t * BLINK_RATE;
                blink_closure(u) > 0.99
            })
            .expect("no fully closed frame found");
        let (shut, shut_lid) = render(t);
        assert!(shut_lid < 0.01, "not closed at {t}: {}", shut_lid);
        let diff = open
            .chunks_exact(4)
            .zip(shut.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(diff > 30, "blink changed only {diff} pixels");
        // 眼睛上方那一行的颜色（资源包里的眼皮色）必须真的出现在画面中。
        let lid = skin
            .image
            .get_pixel(
                (8 + eyes.left.x0) as u32,
                (8 + eyes.left.y0.saturating_sub(1)) as u32,
            )
            .0;
        let lit = lid[0] as f32 + lid[1] as f32 + lid[2] as f32;
        let found = shut
            .chunks_exact(4)
            .filter(|c| c[3] > 200)
            .any(|c| {
                let d = (c[0] as f32 - lid[0] as f32).abs()
                    + (c[1] as f32 - lid[1] as f32).abs()
                    + (c[2] as f32 - lid[2] as f32).abs();
                d < 40. && lit > 0.
            });
        assert!(found, "the eyelid colour never reached the frame");
        // 眨眼之后必须回到完全睁开；腰部轻摆与瞳孔扫视一直在动，
        // 所以这里核对眼睛本身，而不是整帧逐像素相等。
        let (_, again_lid) = render(t + 12.);
        assert!(again_lid > 0.99, "the face must return to its open state");
    }
    /// 闭眼时眼睛区域要被眼皮色整片盖住，不能残留瞳孔或眼白；
    /// 且眼皮取的是眼睛上方那一行，与脸部其余像素同色系，不会出现突兀色块。
    #[test]
    fn eyelid_covers_the_eyes_with_the_row_above_them() {
        let s = Settings::default();
        let skin = blinkable_skin();
        assert!(skin.eyes.is_some(), "the blinkable skin must expose its eyes");
        let mut p = Pose::new(&s);
        let phase = p.blink_phase;
        let mut render = |now: f32| {
            p.update(&s, &skin, Vec2::ZERO, 360., 420., false, now, 0.);
            let mut f = Frame::new(360, 420);
            f.draw(&s, &skin, &p);
            f.rgba()
        };
        let open_time = (0..2400).map(|i| i as f32 / 240.)
            .find(|t| blink_closure(phase + *t * BLINK_RATE) < 0.001).unwrap();
        let open = render(open_time);
        let t = (0..2400)
            .map(|i| i as f32 / 240.)
            .find(|t| blink_closure(phase + *t * BLINK_RATE) > 0.99)
            .expect("no fully closed frame");
        let shut = render(t);
        // 眼皮只会画在头部正面：在「闭眼帧里新增、且位于头部正面矩形内」的像素中
        // 取出现最多的颜色，它必须等于识别格 (4,6) 的像素经正面光照后的颜色。
        let mut tally: Vec<([u8; 3], usize)> = Vec::new();
        for i in 0..360 * 420 {
            if open[i * 4..i * 4 + 4] != shut[i * 4..i * 4 + 4] && shut[i * 4 + 3] > 200 {
                let c = [shut[i * 4], shut[i * 4 + 1], shut[i * 4 + 2]];
                match tally
                    .iter_mut()
                    .find(|(k, _)| (0..3).all(|j| k[j].abs_diff(c[j]) <= 6))
                {
                    Some(e) => e.1 += 1,
                    None => tally.push((c, 1)),
                }
            }
        }
        tally.sort_by_key(|(_, n)| *n);
        // 渲染时正面（face 4）会乘上光照系数，比较时用同一套查色表。
        let k = skin.image.width() / 64;
        let atlas = |x: u32, y: u32| {
            let i = (((y * k + k / 2) * skin.image.width()) + (x * k + k / 2)) as usize;
            skin.colors[i][4]
        };
        let lid = atlas(4, 6);
        // 新增像素里必须出现「眼皮色」：眼皮盖住眼睛时，原来露出的眼睛被
        // 识别格里的眼皮像素取代，正是这块贴图在起作用。
        let find = |c: [u8; 3]| {
            tally
                .iter()
                .find(|(k2, _)| (0..3).all(|j| k2[j].abs_diff(c[j]) <= 10))
                .copied()
        };
        let lid_lit = find(lid).map(|(_, n)| n).unwrap_or(0);
        assert!(
            lid_lit > 15,
            "eyelid colour {lid:?} missing from the blink patch; saw {tally:?}"
        );
    }
    /// 眨眼完全由时间轴决定，不存在累加状态：同一次眨眼在不同帧率下
    /// 求值结果一致，重复调用 update 也不会叠加或漂移。
    #[test]
    fn blink_is_framerate_independent() {
        let skin = blinkable_skin();
        let s = Settings::default();
        let base = Pose::new(&s);
        let phase = base.blink_phase;
        for &target in &[1.0f32, 3.2, 7.5] {
            let mut a = Pose::new(&s);
            a.blink_phase = phase;
            let mut b = Pose::new(&s);
            b.blink_phase = phase;
            let mut t = 0.;
            while t < target {
                t = (t + 1. / 240.).min(target);
                a.update(&s, &skin, Vec2::ZERO, 360., 420., false, t, 1. / 240.);
            }
            b.update(&s, &skin, Vec2::ZERO, 360., 420., false, target, 1. / 60.);
            assert!(
                (a.eyelid - b.eyelid).abs() < 1e-6,
                "eyelid drifted with frame rate at {target}: {} vs {}",
                a.eyelid,
                b.eyelid
            );
        }
    }
    #[test]
    fn held_key_keeps_hand_but_squash_recovers() {
        let (s, skin, mut p) = seated();
        let rest = p.arms[1];
        p.key(81, true, 1., &s);
        tick(&mut p, &s, &skin, 1.08, false);
        assert!((p.squash - 0.95).abs() < 1e-6);
        let held = parts(&s, &skin, &p);
        let board = held.iter().find(|p| p.keyboard).unwrap().transform;
        for i in 10..240 {
            let t = 1. + i as f32 / 120.;
            p.key(81, true, t, &s);
            tick(&mut p, &s, &skin, t, false);
        }
        assert_eq!(p.held_codes(), vec![81]);
        assert_eq!(p.squash, 1.);
        assert!(p.arms[1].angle_between(rest) > 0.05);
        assert_eq!(
            parts(&s, &skin, &p)
                .iter()
                .find(|p| p.keyboard)
                .unwrap()
                .transform,
            board
        );
        let held_pose = p.arms[1];
        for i in 240..360 {
            tick(&mut p, &s, &skin, 1. + i as f32 / 120., false);
        }
        assert!(p.arms[1].angle_between(held_pose) < 0.001);
        p.key(81, false, 4., &s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, 4. + i as f32 / 120., false);
        }
        assert!(p.arms[1].angle_between(rest) < 0.001);
        assert!(p.held.is_empty());
    }
    #[test]
    fn adjustable_squash_is_a_bounded_pulse_even_when_keys_are_held() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for custom in [false, true] {
            for mode in ["precise", "alternating"] {
                for depth in [0., 5., 17., 30.] {
                    let mut s = Settings::default();
                    s.posture = if custom { "custom" } else { "sitting" }.into();
                    s.custom_base = "sitting".into();
                    s.typing_mode = mode.into();
                    s.typing_squash = depth;
                    let mut p = Pose::new(&s);
                    p.key(81, true, 1., &s);
                    tick(&mut p, &s, &skin, 1.08, false);
                    assert!((p.squash - (1. - depth / 100.)).abs() < 1e-6);
                    for i in 1..=40 {
                        let now = 1.08 + i as f32 * 0.01;
                        p.key(81, true, now, &s);
                        tick(&mut p, &s, &skin, now, false);
                        assert!(p.squash >= 1. - depth / 100. - 1e-6 && p.squash <= 1.);
                    }
                    assert_eq!(p.squash, 1.);
                    assert_eq!(p.held_codes(), vec![81]);
                    // New keys can restart the pulse, but never accumulate depth.
                    for (i, vk) in [87, 69, 82, 84, 89].into_iter().enumerate() {
                        let now = 2. + i as f32 * 0.03;
                        p.key(vk, true, now, &s);
                        tick(&mut p, &s, &skin, now + 0.02, false);
                        assert!(p.squash >= 1. - depth / 100. - 1e-6);
                    }
                    s.typing_squash = 0.;
                    tick(&mut p, &s, &skin, 2.15, false);
                    assert_eq!(p.squash, 1.);
                    assert_eq!(p.held_codes().len(), 6);
                }
            }
        }
    }
    #[test]
    fn new_key_restarts_squash_from_full_height_during_both_phases() {
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        for posture in ["sitting", "custom"] {
            for mode in ["precise", "alternating"] {
                for depth in [0., 5., 17., 30.] {
                    for interrupt_after in [0.04, 0.18] {
                        let mut s = Settings::default();
                        s.posture = posture.into();
                        s.custom_base = "sitting".into();
                        s.typing_mode = mode.into();
                        s.typing_squash = depth;
                        let mut p = Pose::new(&s);
                        p.key(81, true, 1., &s);
                        let restart = 1. + interrupt_after;
                        tick(&mut p, &s, &skin, restart, false);
                        if depth > 0. {
                            assert!(p.squash < 1.);
                        }
                        p.key(87, true, restart, &s);
                        assert_eq!(p.squash, 1.);
                        assert_eq!(p.squash_start, Some(restart));
                        assert_eq!(p.squash_pulse(restart), 0.);
                        // A fresh pulse reaches half depth at 40 ms and full at 80 ms.
                        tick(&mut p, &s, &skin, restart + 0.04, false);
                        assert!((p.squash - (1. - depth / 200.)).abs() < 1e-5);
                        p.key(87, true, restart + 0.05, &s);
                        assert_eq!(p.squash_start, Some(restart));
                        tick(&mut p, &s, &skin, restart + 0.08, false);
                        assert!((p.squash - (1. - depth / 100.)).abs() < 1e-5);
                        tick(&mut p, &s, &skin, restart + 0.30, false);
                        assert_eq!(p.squash, 1.);
                        assert_eq!(p.held_codes(), vec![81, 87]);
                        // Releasing and pressing the same key also restarts each time.
                        for i in 0..12 {
                            let now = 2. + i as f32 * 0.03;
                            p.key(87, false, now, &s);
                            p.key(87, true, now, &s);
                            assert_eq!(p.squash, 1.);
                            tick(&mut p, &s, &skin, now + 0.02, false);
                            assert!((p.squash - (1. - depth / 100. * 0.15625)).abs() < 1e-5);
                        }
                        tick(&mut p, &s, &skin, 3., false);
                        assert_eq!(p.squash, 1.);
                        assert_eq!(p.held_codes(), vec![81, 87]);
                    }
                }
            }
        }
    }
    #[test]
    fn multiple_keys_release_to_previous_and_repeat_does_not_alternate() {
        let (mut s, skin, mut p) = seated();
        p.key(81, true, 1., &s);
        for i in 0..60 {
            tick(&mut p, &s, &skin, 1. + i as f32 / 120., false);
        }
        let q = p.arms[1];
        p.key(84, true, 1.5, &s);
        for i in 0..60 {
            tick(&mut p, &s, &skin, 1.5 + i as f32 / 120., false);
        }
        assert!(p.arms[1].angle_between(q) > 0.03);
        p.key(84, false, 2., &s);
        for i in 0..60 {
            tick(&mut p, &s, &skin, 2. + i as f32 / 120., false);
        }
        assert!(p.arms[1].angle_between(q) < 0.001);
        p.reset_keys();
        s.typing_mode = "alternating".into();
        p.key(81, true, 3., &s);
        let side = p.held[0].side;
        for i in 1..30 {
            p.key(81, true, 3. + i as f32 / 30., &s);
        }
        assert_eq!(p.held.len(), 1);
        assert_eq!(p.held[0].side, side);
        p.key(81, false, 4., &s);
        p.key(81, true, 4.01, &s);
        assert_ne!(p.held[0].side, side);
    }
    #[test]
    fn swing_overrides_then_returns_to_live_keyboard_pose() {
        let (s, skin, mut p) = seated();
        p.key(81, true, 1., &s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, 1. + i as f32 / 120., false);
        }
        let pressed = p.arms[1];
        let left = p.arms[0];
        p.click(2., false);
        let mut previous = pressed;
        let mut max_delta = 0f32;
        let mut excursion = 0f32;
        for i in 0..90 {
            tick(&mut p, &s, &skin, 2. + i as f32 / 120., false);
            max_delta = max_delta.max(p.arms[1].angle_between(previous));
            excursion = excursion.max(p.arms[1].angle_between(pressed));
            previous = p.arms[1];
            assert!(p.arms[0].angle_between(left) < 0.001);
        }
        assert!(excursion > 0.3);
        assert!(max_delta < 0.6);
        assert!(p.arms[1].angle_between(pressed) < 0.001);
        assert_eq!(p.held_codes(), vec![81]);
        p.click(3., false);
        p.key(81, false, 3.05, &s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, 3. + i as f32 / 120., false);
        }
        assert!(p.held.is_empty());
        assert!(p.arms[1].angle_between(pressed) > 0.05);
    }
    #[test]
    fn rapid_presses_never_accumulate_scale_or_swing() {
        let (s, skin, mut p) = seated();
        for i in 0..600 {
            let t = 1. + i as f32 / 120.;
            if i % 3 == 0 {
                p.key(81, false, t, &s);
                p.key(81, true, t, &s);
                p.click(t, false);
            }
            tick(&mut p, &s, &skin, t, false);
            assert!((0.94999..=1.00001).contains(&p.squash));
            assert!(p.arms.iter().all(|q| q.is_finite()));
        }
        p.key(81, false, 6., &s);
        for i in 0..120 {
            tick(&mut p, &s, &skin, 6. + i as f32 / 120., false);
        }
        assert_eq!(p.squash, 1.);
        assert!(p.swing_start.is_none());
    }
    #[test]
    fn keyboard() {
        assert!(key_position(112).is_some());
        assert!(key_position(96).is_none());
        assert!(key_position(37).is_none());
        assert!(key_position(81).unwrap().x < 0.);
    }
    #[test]
    fn head_is_immediate() {
        let s = Settings::default();
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut p = Pose::new(&s);
        p.update(
            &s,
            &skin,
            Vec2::new(10000., 10000.),
            360.,
            420.,
            false,
            1.,
            0.016,
        );
        assert!((p.yaw - front_yaw() - s.head_range_x.to_radians()).abs() < 0.00001);
        p.update(
            &s,
            &skin,
            Vec2::new(-10000., -10000.),
            360.,
            420.,
            false,
            1.016,
            0.016,
        );
        assert!((p.yaw - front_yaw() + s.head_range_x.to_radians()).abs() < 0.00001);
    }
    #[test]
    fn pngs() {
        assert!(Skin::load(b"bad").is_err());
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        assert!(!skin.legacy);
    }
    /// 临时预览：把 Q 版站立/坐姿各渲染一帧存成 PNG，供人工核对贴图方向与几何。
    #[test]
    #[ignore]
    fn chibi_preview_png() {
        use image::RgbaImage;
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut s = Settings::default();
        s.chibi = true;
        s.track_mouse = false;
        let mut p = Pose::new(&s);
        for i in 0..60 {
            tick(&mut p, &s, &skin, i as f32 / 60., false);
        }
        let parts = parts_with_cape(&s, &skin, &p, None);
        assert_eq!(parts.len(), 12, "Q 版应生成 12 个 cuboid");
        let mut frame = Frame::new(450, 525);
        frame.draw_ext(&s, &skin, None, &p);
        let img = RgbaImage::from_raw(450, 525, frame.rgba()).unwrap();
        img.save("D:/Minecraft桌宠/work/chibi_stand.png").unwrap();
        s.posture = "sitting".into();
        let mut p2 = Pose::new(&s);
        for i in 0..120 {
            tick(&mut p2, &s, &skin, i as f32 / 120., false);
        }
        let mut f2 = Frame::new(450, 525);
        f2.draw_ext(&s, &skin, None, &p2);
        let img2 = RgbaImage::from_raw(450, 525, f2.rgba()).unwrap();
        img2.save("D:/Minecraft桌宠/work/chibi_sit.png").unwrap();
    }
}
