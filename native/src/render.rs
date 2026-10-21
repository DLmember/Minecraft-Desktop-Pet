use crate::config::Settings;
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use image::RgbaImage;
pub const DEFAULT_SKIN: &[u8] = include_bytes!("../../assets/default-skin.png");
/// 六个面的明暗查色表；皮肤和披风共用同一套光照。
pub fn colors_for(image: &RgbaImage) -> Vec<[[u8; 3]; 6]> {
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

/// 官方披风纹理（64×32，支持有界的等比例高清纹理）。
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
        // Reject oversized dimensions before the PNG decoder allocates pixels.
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        if w < 64 || w > 1024 || w % 64 != 0 || h != w / 2 {
            return Err("披风必须为 64×32 或等比例高清 PNG，最大 1024×512。".into());
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

pub struct Skin {
    pub image: RgbaImage,
    pub legacy: bool,
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
        Ok(Self {
            image,
            legacy,
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
    counter: u32,
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
pub(crate) fn model_transform(s: &Settings) -> Mat4 {
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
        }
    }
    pub fn key(&mut self, vk: u32, down: bool, now: f32, s: &Settings) {
        if !down {
            self.held.retain(|k| k.vk != vk);
            return;
        }
        if !s.keyboard_bounce || !s.keyboard_enabled {
            return;
        }
        // OS auto-repeat must not switch hands, release a held pose or restart squash.
        let already_held = self.held.iter().any(|k| k.vk == vk);
        if !already_held {
            self.squash_start = Some(now);
            self.squash = 1.;
        }
        if !s.is_sitting() || already_held {
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
        let allow_squash = s.keyboard_bounce && s.keyboard_enabled;
        self.squash = if allow_squash {
            1. - self.squash_pulse(now) * s.typing_squash / 100.
        } else {
            1.
        };
        if !s.is_sitting() {
            self.held.clear();
            self.hands = [Hand {
                until: 0.,
                key: None,
            }; 2];
            if !s.keyboard_bounce {
                self.squash_start = None;
                self.squash = 1.;
            }
        }
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
        if s.track_mouse {
            let desired =
                view * Quat::from_rotation_y(horizontal) * Quat::from_rotation_x(vertical);
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
                );
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
                let mut target = Vec3::new(
                    k.map(|v| v.x).unwrap_or(sign * 4.2),
                    if active { 10.35 } else { 11.2 },
                    9.4 + k.map(|v| v.y).unwrap_or(-0.65),
                );
                if !active {
                    target.y += 0.65;
                    target.z = 8.8;
                }
                let arm_body = if s.posture == "custom" {
                    // Swing owns its torso twist; it must not pull the hidden
                    // typing target away and leave a second recovery afterward.
                    body * Mat4::from_quat(custom_body)
                } else {
                    body
                };
                let shoulder = arm_body.transform_point3(Vec3::new(sign * (4. + aw / 2.), 10., 0.));
                let dir = arm_body
                    .inverse()
                    .transform_vector3(target - shoulder)
                    .normalize();
                let typing = Quat::from_rotation_arc(-Vec3::Y, dir);
                typing_target = typing;
                rest = rest.slerp(typing, self.sit);
                if s.posture == "custom" && s.is_sitting() {
                    // Independent idle pose around the usual keyboard-hover position.
                    let idle_target = Vec3::new(sign * 4.2, 11.85, 8.8);
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
        self.base_body() * Mat4::from_rotation_y(self.body_yaw)
    }
}
pub(crate) fn project(v: Vec3, width: f32, height: f32) -> Vec3 {
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
#[derive(Clone)]
struct BoxPart {
    size: Vec3,
    uv: Vec2,
    transform: Mat4,
    inflate: f32,
    mirror: bool,
    keyboard: bool,
    cape: bool,
}
#[allow(dead_code)]
fn parts(s: &Settings, skin: &Skin, pose: &Pose) -> Vec<BoxPart> {
    parts_with_cape(s, skin, pose, None)
}
fn parts_with_cape(
    s: &Settings,
    skin: &Skin,
    pose: &Pose,
    cape: Option<&Cape>,
) -> Vec<BoxPart> {
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
    let mut v = Vec::with_capacity(13);
    let mut part =
        |transform: Mat4, size: Vec3, u: f32, w: f32, outer: Vec2, mirror: bool, hat: bool| {
            v.push(BoxPart {
                size,
                uv: Vec2::new(u, w),
                transform,
                inflate: 0.,
                mirror,
                keyboard: false,
                cape: false,
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
    );
    part(
        pose.base_body()
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
    );
    for side in 0..2 {
        let left = side == 0;
        let sign = if left { 1. } else { -1. };
        let tr = body
            * Mat4::from_translation(Vec3::new(sign * (4. + aw / 2.), 10., 0.))
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
        );
    }
    for side in 0..2 {
        let left = side == 0;
        let sign = if left { 1. } else { -1. };
        let tr = pose.base_body()
            * Mat4::from_translation(Vec3::new(sign * (2. + 0.25 * pose.sit), 0., 0.))
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
        );
    }
    if cape.is_some() {
        // Use the full atlas with its patterned front face pointing away from
        // the body. A negative U offset made the other five faces sample x=0.
        v.push(BoxPart {
            size: Vec3::new(10., 16., 1.),
            uv: Vec2::ZERO,
            transform: body
                * Mat4::from_translation(Vec3::new(0., 3.6, -2.9))
                * Mat4::from_rotation_x(0.14)
                * Mat4::from_rotation_y(std::f32::consts::PI),
            inflate: 0.,
            mirror: false,
            keyboard: false,
            cape: true,
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
    pub fn clear(&mut self) {
        self.pixels.fill(0);
        self.depth.fill(f32::INFINITY);
        self.bounds = [self.width as i32, self.height as i32, 0, 0];
    }
    /// 带披风的绘制；披风只在正版账号取到的时候才传进来。
    pub fn draw_ext(&mut self, s: &Settings, skin: &Skin, cape: Option<&Cape>, pose: &Pose) {
        self.clear();
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
    pub fn draw_keyboard(&mut self, s: &Settings, sit: f32) {
        if sit <= 0.4 {
            return;
        }
        let b = BoxPart {
            size: Vec3::new(19.6, 0.65, 7.8),
            uv: Vec2::ZERO,
            transform: model_transform(s)
                * Mat4::from_translation(Vec3::new(0., 10.2, 9.4))
                * Mat4::from_scale(Vec3::splat(sit))
                * Mat4::from_translation(Vec3::new(0., -0.3, 0.)),
            inflate: 0.,
            mirror: false,
            keyboard: true,
            cape: false,
        };
        let dummy_img = RgbaImage::new(1, 1);
        let dummy_colors = vec![[[128u8; 3]; 6]];
        self.draw_box(&b, Tex::of(&dummy_img, &dummy_colors));
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
                    uv[i] = Vec2::new(
                        x + ix as f32 * tw,
                        y + if f == 3 {
                            (1 - iy) as f32 * th
                        } else {
                            iy as f32 * th
                        },
                    );
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
    }
    fn triangle(&mut self, p: [Vec3; 3], uv: [Vec2; 3], face: usize, b: &BoxPart, skin: Tex) {
        let texscale = skin.image.width() as f32 / 64.;
        let tex = uv.map(|t| t * texscale);
        self.draw_triangle_raw(p, tex, face, b.keyboard, skin.image, skin.colors);
    }
    pub fn draw_triangle(
        &mut self,
        p: [Vec3; 3],
        uv_pixels: [Vec2; 3],
        face: usize,
        image: &RgbaImage,
        colors: &[[[u8; 3]; 6]],
    ) {
        self.draw_triangle_raw(p, uv_pixels, face, false, image, colors);
    }
    fn draw_triangle_raw(
        &mut self,
        p: [Vec3; 3],
        tex: [Vec2; 3],
        face: usize,
        is_keyboard: bool,
        image: &RgbaImage,
        colors: &[[[u8; 3]; 6]],
    ) {
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
                let (color, alpha) = if is_keyboard {
                    (gray, 255)
                } else {
                    let tx = tex[1].x + ta.x * a + tc.x * c;
                    let ty = tex[1].y + ta.y * a + tc.y * c;
                    let xx = (tx as u32).min(image.width() - 1);
                    let yy = (ty as u32).min(image.height() - 1);
                    let ti = (yy * image.width() + xx) as usize;
                    (colors[ti][face], image.get_pixel(xx, yy)[3])
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
pub(crate) fn edge(a: Vec3, b: Vec3, p: Vec3) -> f32 {
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
    fn cape_rejects_oversized_dimensions_before_decode() {
        for (w,h) in [(2048u32,1024u32),(64,64),(63,32),(u32::MAX,32)] {
            let mut bytes = vec![0u8; 24];
            bytes[..8].copy_from_slice(&[137,80,78,71,13,10,26,10]);
            bytes[16..20].copy_from_slice(&w.to_be_bytes());
            bytes[20..24].copy_from_slice(&h.to_be_bytes());
            assert!(Cape::load(&bytes).err().unwrap().contains("最大 1024×512"));
        }
        assert!(Cape::load(&[0;24]).is_err());
        assert!(Cape::load(&vec![0;2*1024*1024+1]).is_err());
    }
    #[test]
    fn cape_uses_all_six_atlas_faces_without_clamping() {
        let mut tex = RgbaImage::new(64,32);
        let regions = [(11,1,1,16),(0,1,1,16),(1,0,10,1),(11,0,10,1),(1,1,10,16),(12,1,10,16)];
        let colors = [[255,0,0,255],[0,255,0,255],[0,0,255,255],
            [255,255,0,255],[255,0,255,255],[0,255,255,255]];
        for (i,(x,y,w,h)) in regions.into_iter().enumerate() {
            for yy in y..y+h { for xx in x..x+w { tex.put_pixel(xx,yy,image::Rgba(colors[i])); } }
        }
        let mut png = std::io::Cursor::new(Vec::new());
        tex.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let cape = Cape::load(png.get_ref()).unwrap();
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        let s = Settings::default(); let pose = Pose::new(&s);
        let parts = parts_with_cape(&s,&skin,&pose,Some(&cape));
        let part = parts.iter().find(|p| p.cape).unwrap();
        assert_eq!(part.uv, Vec2::ZERO);
        // +Z is the outward pattern after rotation; -Z is the lining.
        let direction = part.transform.transform_vector3(Vec3::Z);
        assert!(direction.z < -0.9);
        assert_eq!(cape.image.get_pixel(5,8).0, colors[4]);
        assert_eq!(cape.image.get_pixel(16,8).0, colors[5]);
    }
    #[test]
    fn cape_renders_from_the_official_texture_region() {
        use image::{ImageFormat, Rgba, RgbaImage};
        let skin = Skin::load(DEFAULT_SKIN).unwrap();
        // Isolate the outward pattern; other faces are tested separately.
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
            ("cape", 10., 16., 1., 0., 0.),
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
}
