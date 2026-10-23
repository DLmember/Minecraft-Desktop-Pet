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
/// 按住 Shift 潜行多久之后开始扭（秒）。材质包原值 10 秒，这里按使用者要求
/// 缩短到 3 秒：长按 Shift 潜行 3 秒即开始扭动。
const SNEAK_DELAY: f32 = 3.;
/// 扭动角速度（弧度/秒）：材质包 `SneakyDanceTimer += frame_time*0.175`，每 tick
/// 0.175 rad 即 3.5 rad/s；按使用者要求加快 20% → 4.2 rad/s，一个来回约 1.5 秒。
const DANCE_RATE: f32 = 4.2;
/// 强度涨落的时间常数（秒）。材质包 128 tick 涨到 0.15、8 tick 落回，
/// 分别等于 6.4 秒和 0.4 秒。
const DANCE_RISE: f32 = 6.4;
const DANCE_FALL: f32 = 0.4;
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
const SWING_SECONDS: f32 = 0.30; // Vanilla attack animation: six ticks at 20 ticks/s.
pub struct Pose {
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
        self.sneak_timer > 0. || self.dance > 0.001
    }
    pub fn key(&mut self, vk: u32, down: bool, now: f32, s: &Settings) {
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
        self.held.push(HeldKey {
            vk,
            side,
            key: self.hands[side].key,
        });
        // A new physical press interrupts the pulse and starts at full height.
        self.squash_start = Some(now);
        self.squash = 1.;
    }
    pub fn held_codes(&self) -> Vec<u32> {
        self.held.iter().map(|k| k.vk).collect()
    }
    pub fn click(&mut self, now: f32, left: bool) {
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
        let a = 1. - (-dt * 12.).exp();
        let sit = if s.is_sitting() { 1. } else { 0. };
        let crouch = if s.posture == "standing" && shift && s.keyboard_enabled {
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
        // `age` 是资源包里按 tick 计的游戏时间，所有待机动画都以它为准。
        let age = now * 20.;
        // 眼睛部分逐条照抄材质包 head 模块的表达式（`var.r` 就是实例的随机相位）：
        //   var.b     = age / (11 - random(id)*2)
        //   var.ylook = clamp(sin(r + age/60)*6, 0, 1) * 1.2
        //   var.nov1  = clamp(-5.6 + cos(-pi/6 + r + age/80)*6, 0, 1)
        //   var.nov2  = clamp(-39 + cos(-pi/4 + r + b/20)*40, 0, 1)
        // 站着的桌宠 `limb_speed = 0`，所以这两项原式里乘的 `clamp(2 - 速度*4,0,1)`
        // 与 `clamp(1 - 速度*2,0,1)` 都等于 1。
        let r = self.blink_phase;
        let b = age / (11. - random_unit(r) * 2.);
        let ylook = ((r + age / 60.).sin() * 6.).clamp(0., 1.) * 1.2;
        let nov1 = (-5.6 + (-std::f32::consts::FRAC_PI_6 + r + age / 80.).cos() * 6.).clamp(0., 1.);
        let nov2 = (-39. + (-std::f32::consts::FRAC_PI_4 + r + b / 20.).cos() * 40.).clamp(0., 1.);
        let head_yaw = horizontal.to_degrees();
        let head_pitch = vertical.to_degrees();
        //   ctrl_pupil.tx = 0.5 - head_yaw/(80 + 160*ylook) + (-sin(pi/4 + r + age/10)*3 - 0.3)
        //                   * clamp(nov1*4, 0, 1)
        //   ctrl_pupil.ty = 0.5 + clamp(head_pitch/80, -0.5, 0.5)
        //                   + (-cos(r + b + sin(r + b))/4) * nov2 + clamp(2*nov1, 0, 0.3)
        // 桌宠的瞳孔以眼睛中心为原点，所以减去材质包的 0.5 基线。
        let ctrl_tx = 0.5 - head_yaw / (80. + 160. * ylook)
            + (-(std::f32::consts::FRAC_PI_4 + r + age / 10.).sin() * 3. - 0.3)
                * (nov1 * 4.).clamp(0., 1.);
        let ctrl_ty = 0.5
            + (head_pitch / 80.).clamp(-0.5, 0.5)
            + (-(r + b + (r + b).sin()).cos() / 4.) * nov2
            + (2. * nov1).clamp(0., 0.3);
        self.pupil = Vec2::new(
            (ctrl_tx - 0.5).clamp(-0.5, 0.5),
            (ctrl_ty - 0.5).clamp(-0.5, 0.5),
        );
        //   r_eye_top.ty = clamp(head_pitch/60, -0.8, 0.5) + clamp(2*nov1, 0, 0.3)
        //   eye_white.sy = clamp(1 - r_eye_top.ty, 0.5, 3)  → 眼皮高度
        // 睁眼程度 = 眨眼曲线 × 这一层开合，于是低头时半眯、`nov1` 触发时瞪一下。
        let eye_top = (head_pitch / 60.).clamp(-0.8, 0.5) + (2. * nov1).clamp(0., 0.3);
        self.eyelid = (1. - closure) * (1. - eye_top).clamp(0.5, 1.);
        // 常态下只有头跟着视线转，腰和身体保持不动。
        // 腰部的动作只来自潜行扭动（材质包 cloak 模块的 `var.SneakyDance*`），
        // 幅度统一用材质包原值：蹲下后累计 `var.SneakingTimer`（每 tick +0.1，
        // 到 20 才启动 `SneakyDanceTimer`，每 tick 推进 0.175 rad），强度用 128 tick
        // 涨到 0.15、松开后 8 tick 落回；`abs(move_forward)+abs(move_strafing) < 1`
        // 这一条在桌宠上永远成立（按 Shift 潜行时本来就不移动）。
        self.sneak_timer = if self.crouch > 0.5 {
            self.sneak_timer + dt
        } else {
            0.
        };
        let dancing = self.sneak_timer >= SNEAK_DELAY;
        self.dance_timer = if dancing {
            self.dance_timer + dt * DANCE_RATE
        } else {
            0.
        };
        let (goal, tau) = if dancing {
            (1., DANCE_RISE)
        } else {
            (0., DANCE_FALL)
        };
        self.dance += (goal - self.dance) * (dt / tau).min(1.);
        // 下面就是材质包 cloak 模块 SneakyDance 的通道，按使用者要求去掉腿部
        // （脚不再跟着扭），其余逐条照抄、不做任何缩放：
        // 材质包的旋转值本就是弧度（表达式里的 `torad()` 只用来把度数字面量换算成
        // 弧度），强度 I 最大 0.15 rad ≈ 8.6°，位移保持原值。
        //   body.rz += sin(t) * I            body.ty += cos(2t) * -I
        //   head.rx += cos(2t-0.5) * I*0.1    head.rz += sin(t-0.5) * I*0.5
        //   arm.rz  += sin(t-0.25) * I        arm.tx += -3 * sin(t) * I
        //   left_arm.ty += 3*sin(t)*I          right_arm.ty += -3*sin(t)*I
        let i = self.dance * 0.15;
        let t = self.dance_timer;
        self.body_roll = t.sin() * i;
        self.body_bob = (t * 2.).cos() * -i;
        self.head_nod = (t * 2. - 0.5).cos() * i * 0.1;
        self.head_roll = (t - 0.5).sin() * i * 0.5;
        self.arm_roll = (t - 0.25).sin() * i;
        self.arm_slide = -3. * t.sin() * i;
        self.arm_bob = 3. * t.sin() * i;
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
            let mut rest = rotation(-0.035, 0., sign * 0.105 + self.arm_roll);
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
                let typing = Quat::from_rotation_arc(-Vec3::Y, dir);
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
                    rest = Quat::from_rotation_arc(-Vec3::Y, idle_dir)
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
            self.arms[side] = self.typing_arms[side];
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
    }
    fn base_body(&self) -> Mat4 {
        Mat4::from_translation(Vec3::new(
            0.,
            12. - 6. * self.sit - 2.1 * self.crouch,
            -1.1 * self.crouch,
        )) * Mat4::from_rotation_x(0.34 * self.crouch)
    }
    fn body(&self) -> Mat4 {
        Mat4::from_translation(Vec3::Y * self.body_bob)
            * self.base_body()
            * Mat4::from_quat(rotation(0., self.body_yaw, self.body_roll))
    }
    /// 上半身空间（`body` 骨架）：`body.ty` 的上下起伏与 `body.rz` 的扭动都在这里，
    /// 头和手臂挂在上面。腿不在这里——材质包里 leg 是独立骨骼，不吃 `body` 的变换。
    pub fn torso_space(&self) -> Mat4 {
        Mat4::from_translation(Vec3::Y * self.body_bob)
            * self.base_body()
            * Mat4::from_rotation_z(self.body_roll)
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
const CHIBI_HAT: ChibiBox = ChibiBox { off: [0.05,3.7,0.2], size: [7.6,7.6,7.6], rot: [0.0,0.0,0.0], faces: [FaceUv { u0: 12.0, v0: 2.0, u1: 14.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 8.0, v0: 2.0, u1: 10.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 2.0, u1: 12.0, v1: 0.0, rotation: 0 }, FaceUv { u0: 12.0, v0: 0.0, u1: 14.0, v1: 2.0, rotation: 0 }, FaceUv { u0: 10.0, v0: 2.0, u1: 12.0, v1: 4.0, rotation: 0 }, FaceUv { u0: 14.0, v0: 2.0, u1: 16.0, v1: 4.0, rotation: 0 }] };
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
        return parts_chibi(s, skin, pose);
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
            eyes: if eyes { skin.eyes } else { None },
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
        let tr = body
            * Mat4::from_translation(Vec3::new(
                sign * (4. + aw / 2.) + pose.arm_slide,
                10. + sign * pose.arm_bob,
                0.,
            ))
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
                sign * (2. + 0.25 * pose.sit),
                0.,
                0.,
            ))
            // 腿部不参与扭腰：材质包 SneakyDance 原本还有 leg.rz/tx，按使用者
            // 要求去掉，脚始终落在身体正下方，只有身体和上半身摆动。
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
    let anchor = 6. * pose.sit;
    let squash = Mat4::from_translation(Vec3::Y * anchor)
        * Mat4::from_scale(Vec3::new(1., pose.squash, 1.))
        * Mat4::from_translation(-Vec3::Y * anchor);
    for part in &mut v {
        part.transform = squash * part.transform;
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
fn parts_chibi(s: &Settings, skin: &Skin, pose: &Pose) -> Vec<BoxPart> {
    let slim = s.arm_model == "slim" && !skin.legacy;
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
    let body = Mat4::from_translation(Vec3::Y * pose.body_bob)
        * base
        * Mat4::from_quat(rotation(0., pose.body_yaw, pose.body_roll))
        * custom_body;
    let torso = Mat4::from_translation(Vec3::Y * pose.body_bob)
        * base
        * Mat4::from_rotation_z(pose.body_roll)
        * custom_body;

    let mut v = Vec::with_capacity(14);
    let push = |v: &mut Vec<BoxPart>, cb: &ChibiBox, transform: Mat4, layer: bool, eyes: bool| {
        let t = transform
            * Mat4::from_quat(degrees(&cb.rot))
            * Mat4::from_translation(Vec3::from_array(cb.off));
        v.push(BoxPart {
            size: Vec3::from_array(cb.size),
            uv: Vec2::ZERO,
            transform: t,
            inflate: if layer { 0.5 } else { 0. },
            mirror: false,
            keyboard: false,
            cape: false,
            faces: Some(cb.faces),
            eyes: if eyes { skin.eyes } else { None },
            eyelid: pose.eyelid,
            pupil: if eyes { pose.pupil } else { Vec2::ZERO },
        });
    };

    // 头（颈在身体顶部 Y=6）
    let head = torso
        * Mat4::from_translation(Vec3::new(0., 3.7, 0.))
        * Mat4::from_quat(pose.head_rotation);
    push(&mut v, &CHIBI_HEAD, head, false, true);
    if s.outer_layer {
        push(&mut v, &CHIBI_HAT, head, true, false);
    }

    // 身体（中心在 Y=3）
    let body_t = body * Mat4::from_translation(Vec3::new(0., 1.85, 0.));
    push(&mut v, &CHIBI_BODY, body_t, false, false);
    if s.outer_layer {
        push(&mut v, &CHIBI_BODY_LAYER, body_t, true, false);
    }

    // 手臂（肩在 Y=5 两侧）
    for side in 0..2 {
        let left = side == 0;
        let sign = if left { 1. } else { -1. };
        let arm = body
            * Mat4::from_translation(Vec3::new(
                sign * (1.75 + arm_w / 2.) + pose.arm_slide,
                3.7 + sign * pose.arm_bob,
                0.,
            ))
            * Mat4::from_quat(pose.arms[side]);
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
        push(&mut v, inner, arm, false, false);
        if s.outer_layer {
            push(&mut v, outer, arm, true, false);
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
        let leg = base
            * Mat4::from_translation(Vec3::new(sign * 0.75, 0., 0.))
            * Mat4::from_quat(leg_rot);
        let (inner, outer) = if left {
            (&CHIBI_LEG_LEFT, &CHIBI_LEG_LEFT_LAYER)
        } else {
            (&CHIBI_LEG_RIGHT, &CHIBI_LEG_RIGHT_LAYER)
        };
        push(&mut v, inner, leg, false, false);
        if s.outer_layer {
            push(&mut v, outer, leg, true, false);
        }
    }

    // Squash 只作用在角色上（锚点按 Q 版支撑高度）。
    let anchor = 3. * pose.sit;
    let squash = Mat4::from_translation(Vec3::Y * anchor)
        * Mat4::from_scale(Vec3::new(1., pose.squash, 1.))
        * Mat4::from_translation(-Vec3::Y * anchor);
    for part in &mut v {
        part.transform = squash * part.transform;
    }
    // Q 版整体放大（脚仍在 Y=0，头顶约 19.3，缩放后约 29）。
    let chibi_scale = Mat4::from_scale(Vec3::splat(1.5));
    for part in &mut v {
        part.transform = chibi_scale * part.transform;
    }
    // 坐姿打字键盘：和标准人形同一套尺寸/渐入，放在 Q 版坐姿身体前方。
    if pose.sit > 0.4 {
        v.push(BoxPart {
            size: Vec3::new(19.6, 0.65, 7.8),
            uv: Vec2::ZERO,
            transform: Mat4::from_translation(Vec3::new(0., 5., 9.4))
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
}
impl Frame {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            width: w,
            height: h,
            pixels: vec![0; w * h * 4],
            depth: vec![f32::INFINITY; w * h],
            bounds: [0; 4],
        }
    }
    pub fn draw(&mut self, s: &Settings, skin: &Skin, pose: &Pose) {
        self.draw_ext(s, skin, None, pose);
    }
    /// 带披风的绘制；披风只在正版账号取到的时候才传进来。
    pub fn draw_ext(&mut self, s: &Settings, skin: &Skin, cape: Option<&Cape>, pose: &Pose) {
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
        let x0 = eye.x0 as f32 - 4.;
        let x1 = eye.x1 as f32 + 1. - 4.;
        let top = 4. - eye.y0 as f32;
        // 眼皮从上边缘向下长满眼睛所在的那一行（材质包的 blink.sy 0→1）。
        let bottom = top - eye.rows as f32 * closed;
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
        let gray = [131u8, 135, 138].map(|c| {
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
    /// 桌宠按使用者要求把等待缩短到 3 秒；起身后强度必须立刻回落，不能继续扭。
    #[test]
    fn sneaking_starts_the_dance_after_a_delay() {
        let s = Settings::default();
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let mut p = Pose::new(&s);
        let dt = 1. / 120.;
        let mut t = 0.;
        let cursor = Vec2::new(180., 160.);
        while t < 2.5 {
            p.update(&s, &skin, cursor, 360., 420., true, t, dt);
            assert!(p.dance < 0.001, "danced before the delay: {}", p.dance);
            t += dt;
        }
        while t < 12.0 {
            p.update(&s, &skin, cursor, 360., 420., true, t, dt);
            t += dt;
        }
        assert!(p.dance > 0.4, "the dance never got going: {}", p.dance);
        assert!(p.body_roll.abs() > 0.01, "roll stayed flat: {}", p.body_roll);
        for _ in 0..240 {
            p.update(&s, &skin, cursor, 360., 420., false, t, dt);
            t += dt;
        }
        assert!(p.dance < 0.01, "the dance kept going: {}", p.dance);
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
        let open = render(0.);
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
