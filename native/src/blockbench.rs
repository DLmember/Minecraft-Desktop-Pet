#![allow(dead_code)]
use crate::config::Settings;
use crate::render::{colors_for, model_transform, project, Frame};
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use image::{ImageFormat, Rgba, RgbaImage};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interpolation {
    Linear,
    Step,
    Smooth,
}

#[derive(Clone, Debug)]
pub struct Keyframe<T> {
    pub time: f32,
    pub value: T,
    pub interpolation: Interpolation,
}

#[derive(Clone, Debug, Default)]
pub struct BbBoneAnimator {
    pub rotations: Vec<Keyframe<Vec3>>,
    pub positions: Vec<Keyframe<Vec3>>,
    pub scales: Vec<Keyframe<Vec3>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BbLoopMode {
    Loop,
    Once,
    Hold,
}

#[derive(Clone, Debug)]
pub struct BbAnimation {
    pub name: String,
    pub length: f32,
    pub loop_mode: BbLoopMode,
    pub animators: HashMap<String, BbBoneAnimator>,
}

#[derive(Clone, Debug)]
pub struct BbFace {
    pub uv: [f32; 4], // [u1, v1, u2, v2] in pixels
    pub texture: usize,
    pub rotation: i32,
}

#[derive(Clone, Debug)]
pub struct BbElement {
    pub uuid: String,
    pub name: String,
    pub from: Vec3,
    pub to: Vec3,
    pub origin: Vec3,
    pub rotation: Vec3, // Euler angles in degrees
    pub inflate: f32,
    pub mirror: bool,
    pub faces: [Option<BbFace>; 6], // 0: East(+X), 1: West(-X), 2: Up(+Y), 3: Down(-Y), 4: South(+Z), 5: North(-Z)
}

#[derive(Clone, Debug)]
pub struct BbBone {
    pub uuid: String,
    pub name: String,
    pub origin: Vec3,
    pub rotation: Vec3, // Authored Euler angles in degrees
    pub parent: Option<usize>,
    pub children_bones: Vec<usize>,
    pub children_elements: Vec<usize>,
}

#[derive(Clone)]
pub struct BbTexture {
    pub id: String,
    pub name: String,
    pub image: RgbaImage,
    pub colors: Vec<[[u8; 3]; 6]>,
}

#[derive(Clone)]
pub struct BbModel {
    pub name: String,
    pub resolution: Vec2,
    pub elements: Vec<BbElement>,
    pub bones: Vec<BbBone>,
    pub root_bones: Vec<usize>,
    pub root_elements: Vec<usize>,
    pub textures: Vec<BbTexture>,
    pub animations: Vec<BbAnimation>,
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
            return Err("无效 Base64 数据".into());
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        bits = (bits << 6) | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            result.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    Ok(result)
}

fn parse_f32(val: &Value) -> Option<f32> {
    if let Some(n) = val.as_f64() {
        return Some(n as f32);
    }
    if let Some(s) = val.as_str() {
        if let Ok(v) = s.trim().parse::<f32>() {
            return Some(v);
        }
    }
    None
}

fn parse_vec3(val: &Value) -> Option<Vec3> {
    if let Some(arr) = val.as_array() {
        if arr.len() >= 3 {
            let x = parse_f32(&arr[0]).unwrap_or(0.0);
            let y = parse_f32(&arr[1]).unwrap_or(0.0);
            let z = parse_f32(&arr[2]).unwrap_or(0.0);
            return Some(Vec3::new(x, y, z));
        }
    }
    if let Some(obj) = val.as_object() {
        if let Some(vec) = obj.get("vector").and_then(|v| v.as_array()) {
            if vec.len() >= 3 {
                let x = parse_f32(&vec[0]).unwrap_or(0.0);
                let y = parse_f32(&vec[1]).unwrap_or(0.0);
                let z = parse_f32(&vec[2]).unwrap_or(0.0);
                return Some(Vec3::new(x, y, z));
            }
        }
        let x = obj.get("x").and_then(parse_f32).unwrap_or(0.0);
        let y = obj.get("y").and_then(parse_f32).unwrap_or(0.0);
        let z = obj.get("z").and_then(parse_f32).unwrap_or(0.0);
        return Some(Vec3::new(x, y, z));
    }
    None
}

fn default_texture() -> BbTexture {
    let mut image = RgbaImage::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            let c = if ((x / 4) + (y / 4)) % 2 == 0 { 200 } else { 240 };
            image.put_pixel(x, y, Rgba([c, c, c, 255]));
        }
    }
    let colors = colors_for(&image);
    BbTexture {
        id: "default".into(),
        name: "default.png".into(),
        image,
        colors,
    }
}

impl BbModel {
    pub fn load_file(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("无法读取文件: {e}"))?;
        let base_dir = path.parent();
        Self::load(&bytes, base_dir)
    }

    pub fn load(bytes: &[u8], base_dir: Option<&Path>) -> Result<Self, String> {
        let json: Value = serde_json::from_slice(bytes)
            .map_err(|e| format!("模型文件不是合法的 JSON: {e}"))?;

        if json.get("minecraft:geometry").is_some() {
            return Self::load_bedrock_geometry(&json, base_dir);
        }

        Self::load_bbmodel(&json, base_dir)
    }

    fn load_bbmodel(json: &Value, base_dir: Option<&Path>) -> Result<Self, String> {
        let name = json["name"]
            .as_str()
            .or_else(|| json["geometry_name"].as_str())
            .unwrap_or("Blockbench Model")
            .to_owned();

        let tex_w = json["resolution"]["width"].as_f64().unwrap_or(64.0) as f32;
        let tex_h = json["resolution"]["height"].as_f64().unwrap_or(64.0) as f32;
        let resolution = Vec2::new(tex_w.max(1.0), tex_h.max(1.0));

        // 1. Textures
        let mut textures = Vec::new();
        if let Some(tex_arr) = json["textures"].as_array() {
            for (idx, tex_val) in tex_arr.iter().enumerate() {
                let id = tex_val["id"]
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| tex_val["id"].as_i64().map(|i| i.to_string()))
                    .or_else(|| tex_val["uuid"].as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| idx.to_string());
                let t_name = tex_val["name"].as_str().unwrap_or("texture").to_string();

                let source = tex_val["source"].as_str().unwrap_or("");
                let mut img = None;

                if source.starts_with("data:") || source.contains(";base64,") {
                    if let Some(pos) = source.find(',') {
                        let b64 = &source[pos + 1..];
                        if let Ok(raw) = decode64(b64) {
                            if let Ok(decoded) = image::load_from_memory_with_format(&raw, ImageFormat::Png) {
                                img = Some(decoded.into_rgba8());
                            }
                        }
                    }
                } else if !source.is_empty() {
                    let candidates = [
                        PathBuf::from(source),
                        base_dir.map(|d| d.join(source)).unwrap_or_default(),
                        base_dir.map(|d| d.join(&t_name)).unwrap_or_default(),
                    ];
                    for cand in &candidates {
                        if cand.exists() {
                            if let Ok(b) = std::fs::read(cand) {
                                if let Ok(decoded) = image::load_from_memory(&b) {
                                    img = Some(decoded.into_rgba8());
                                    break;
                                }
                            }
                        }
                    }
                }

                if let Some(image) = img {
                    let colors = colors_for(&image);
                    textures.push(BbTexture {
                        id,
                        name: t_name,
                        image,
                        colors,
                    });
                }
            }
        }
        if textures.is_empty() {
            textures.push(default_texture());
        }

        // Texture UUID/ID mapping to texture index
        let mut tex_map = HashMap::new();
        for (i, t) in textures.iter().enumerate() {
            tex_map.insert(t.id.clone(), i);
            tex_map.insert(t.name.clone(), i);
        }

        // 2. Elements
        let mut elements = Vec::new();
        let mut elem_uuid_map = HashMap::new();

        if let Some(elem_arr) = json["elements"].as_array() {
            for (idx, el) in elem_arr.iter().enumerate() {
                let uuid = el["uuid"].as_str().unwrap_or(&idx.to_string()).to_string();
                let e_name = el["name"].as_str().unwrap_or("cube").to_string();

                let from = parse_vec3(&el["from"]).unwrap_or(Vec3::ZERO);
                let to = parse_vec3(&el["to"]).unwrap_or(Vec3::ONE);
                let origin = parse_vec3(&el["origin"]).unwrap_or((from + to) * 0.5);
                let rotation = parse_vec3(&el["rotation"]).unwrap_or(Vec3::ZERO);
                let inflate = parse_f32(&el["inflate"]).unwrap_or(0.0);
                let mirror = el["mirror"].as_bool().unwrap_or(false);
                let box_uv = el["box_uv"].as_bool().unwrap_or(false);
                let uv_offset = parse_vec3(&el["uv_offset"])
                    .map(|v| Vec2::new(v.x, v.y))
                    .unwrap_or(Vec2::ZERO);

                let mut faces = [None, None, None, None, None, None];
                // Face indices: 0: East(+X), 1: West(-X), 2: Up(+Y), 3: Down(-Y), 4: South(+Z), 5: North(-Z)
                let face_names = ["east", "west", "up", "down", "south", "north"];

                if box_uv {
                    let w = (to.x - from.x).abs();
                    let h = (to.y - from.y).abs();
                    let d = (to.z - from.z).abs();
                    let u0 = uv_offset.x;
                    let v0 = uv_offset.y;

                    // Standard Box UV mapping (matching Minecraft texture layout)
                    let box_coords = [
                        [u0 + d + w, v0 + d, u0 + 2.0 * d + w, v0 + d + h], // East
                        [u0, v0 + d, u0 + d, v0 + d + h],                     // West
                        [u0 + d, v0, u0 + d + w, v0 + d],                     // Up
                        [u0 + d + w, v0, u0 + 2.0 * w + d, v0 + d],           // Down
                        [u0 + d, v0 + d, u0 + d + w, v0 + d + h],             // South
                        [u0 + 2.0 * d + w, v0 + d, u0 + 2.0 * d + 2.0 * w, v0 + d + h], // North
                    ];

                    for (f_idx, &coords) in box_coords.iter().enumerate() {
                        let mut u1 = coords[0];
                        let mut u2 = coords[2];
                        if mirror {
                            std::mem::swap(&mut u1, &mut u2);
                        }
                        faces[f_idx] = Some(BbFace {
                            uv: [u1, coords[1], u2, coords[3]],
                            texture: 0,
                            rotation: 0,
                        });
                    }
                }

                if let Some(faces_obj) = el["faces"].as_object() {
                    for (f_idx, &fname) in face_names.iter().enumerate() {
                        if let Some(f_val) = faces_obj.get(fname) {
                            if f_val.is_null() {
                                faces[f_idx] = None;
                                continue;
                            }
                            let uvs = f_val["uv"].as_array();
                            let uv = if let Some(a) = uvs {
                                if a.len() >= 4 {
                                    [
                                        parse_f32(&a[0]).unwrap_or(0.0),
                                        parse_f32(&a[1]).unwrap_or(0.0),
                                        parse_f32(&a[2]).unwrap_or(0.0),
                                        parse_f32(&a[3]).unwrap_or(0.0),
                                    ]
                                } else {
                                    [0.0, 0.0, 16.0, 16.0]
                                }
                            } else {
                                [0.0, 0.0, 16.0, 16.0]
                            };

                            let tex_idx = if let Some(tex_num) = f_val["texture"].as_i64() {
                                (tex_num as usize).min(textures.len().saturating_sub(1))
                            } else if let Some(tex_str) = f_val["texture"].as_str() {
                                tex_map.get(tex_str).copied().unwrap_or(0)
                            } else {
                                0
                            };

                            let rot = f_val["rotation"].as_i64().unwrap_or(0) as i32;

                            faces[f_idx] = Some(BbFace {
                                uv,
                                texture: tex_idx,
                                rotation: rot,
                            });
                        }
                    }
                }

                elem_uuid_map.insert(uuid.clone(), elements.len());
                elem_uuid_map.insert(e_name.clone(), elements.len());
                elements.push(BbElement {
                    uuid,
                    name: e_name,
                    from,
                    to,
                    origin,
                    rotation,
                    inflate,
                    mirror,
                    faces,
                });
            }
        }

        // 3. Outliner & Bones
        let mut bones = Vec::new();
        let mut bone_uuid_map = HashMap::new();
        let mut root_bones = Vec::new();
        let mut assigned_elements = vec![false; elements.len()];

        // Parse Blockbench 5 groups array if present
        if let Some(groups_arr) = json["groups"].as_array() {
            for g in groups_arr {
                let uuid = g["uuid"].as_str().unwrap_or("").to_string();
                let b_name = g["name"].as_str().unwrap_or("bone").to_string();
                let origin = parse_vec3(&g["origin"]).unwrap_or(Vec3::ZERO);
                let rotation = parse_vec3(&g["rotation"]).unwrap_or(Vec3::ZERO);

                let idx = bones.len();
                if !uuid.is_empty() {
                    bone_uuid_map.insert(uuid.clone(), idx);
                }
                bone_uuid_map.insert(b_name.clone(), idx);
                bones.push(BbBone {
                    uuid,
                    name: b_name,
                    origin,
                    rotation,
                    parent: None,
                    children_bones: Vec::new(),
                    children_elements: Vec::new(),
                });
            }
        }

        // Parse Outliner tree (Blockbench 3/4 or 5)
        fn parse_outliner_item(
            item: &Value,
            parent_bone: Option<usize>,
            bones: &mut Vec<BbBone>,
            bone_uuid_map: &mut HashMap<String, usize>,
            elem_uuid_map: &HashMap<String, usize>,
            assigned_elements: &mut [bool],
            root_bones: &mut Vec<usize>,
        ) {
            if let Some(uuid_str) = item.as_str() {
                // Element UUID or Bone UUID reference
                if let Some(&e_idx) = elem_uuid_map.get(uuid_str) {
                    if let Some(b_idx) = parent_bone {
                        bones[b_idx].children_elements.push(e_idx);
                        assigned_elements[e_idx] = true;
                    }
                } else if let Some(&b_idx) = bone_uuid_map.get(uuid_str) {
                    bones[b_idx].parent = parent_bone;
                    if let Some(p) = parent_bone {
                        bones[p].children_bones.push(b_idx);
                    } else {
                        root_bones.push(b_idx);
                    }
                }
                return;
            }

            if let Some(obj) = item.as_object() {
                let uuid = obj.get("uuid").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let b_name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("bone").to_string();
                let origin = obj.get("origin").and_then(parse_vec3).unwrap_or(Vec3::ZERO);
                let rotation = obj.get("rotation").and_then(parse_vec3).unwrap_or(Vec3::ZERO);

                let existing = if !uuid.is_empty() {
                    bone_uuid_map.get(&uuid).copied()
                } else {
                    None
                };

                let b_idx = if let Some(existing) = existing {
                    bones[existing].parent = parent_bone;
                    existing
                } else {
                    let new_idx = bones.len();
                    if !uuid.is_empty() {
                        bone_uuid_map.insert(uuid.clone(), new_idx);
                    }
                    bone_uuid_map.insert(b_name.clone(), new_idx);
                    bones.push(BbBone {
                        uuid: uuid.clone(),
                        name: b_name,
                        origin,
                        rotation,
                        parent: parent_bone,
                        children_bones: Vec::new(),
                        children_elements: Vec::new(),
                    });
                    new_idx
                };

                if let Some(p) = parent_bone {
                    bones[p].children_bones.push(b_idx);
                } else {
                    root_bones.push(b_idx);
                }

                if let Some(children) = obj.get("children").and_then(|v| v.as_array()) {
                    for ch in children {
                        parse_outliner_item(
                            ch,
                            Some(b_idx),
                            bones,
                            bone_uuid_map,
                            elem_uuid_map,
                            assigned_elements,
                            root_bones,
                        );
                    }
                }
            }
        }

        if let Some(outliner_arr) = json["outliner"].as_array() {
            for item in outliner_arr {
                parse_outliner_item(
                    item,
                    None,
                    &mut bones,
                    &mut bone_uuid_map,
                    &elem_uuid_map,
                    &mut assigned_elements,
                    &mut root_bones,
                );
            }
        }

        // Elements not in any bone are root elements
        let mut root_elements = Vec::new();
        for (i, &assigned) in assigned_elements.iter().enumerate() {
            if !assigned {
                root_elements.push(i);
            }
        }

        // 4. Animations
        let mut animations = Vec::new();
        if let Some(anim_arr) = json["animations"].as_array() {
            for a_val in anim_arr {
                let a_name = a_val["name"].as_str().unwrap_or("animation").to_string();
                let length = parse_f32(&a_val["length"]).unwrap_or(0.0);
                let loop_str = a_val["loop"].as_str().unwrap_or("loop");
                let loop_mode = match loop_str {
                    "once" => BbLoopMode::Once,
                    "hold" => BbLoopMode::Hold,
                    _ => BbLoopMode::Loop,
                };

                let mut animators = HashMap::new();

                if let Some(animators_obj) = a_val["animators"].as_object() {
                    for (bone_key, b_anim) in animators_obj {
                        let bone_name = b_anim["name"].as_str().unwrap_or(bone_key).to_string();
                        let mut bone_anim = BbBoneAnimator::default();

                        // Collect keyframes from both "keyframes" array and "channels" array
                        let mut parse_kfs = |kfs_arr: &Vec<Value>, ch_override: Option<&str>| {
                            for kf in kfs_arr {
                                let ch = ch_override
                                    .or_else(|| kf["channel"].as_str())
                                    .unwrap_or("");
                                let time = parse_f32(&kf["time"]).unwrap_or(0.0);
                                let interp_str = kf["interpolation"].as_str().unwrap_or("linear");
                                let interpolation = match interp_str {
                                    "step" => Interpolation::Step,
                                    "catmullrom" | "bezier" | "smooth" => Interpolation::Smooth,
                                    _ => Interpolation::Linear,
                                };

                                let val = if let Some(dps) = kf["data_points"].as_array() {
                                    dps.first().and_then(parse_vec3).unwrap_or(Vec3::ZERO)
                                } else if let Some(dp) = kf.get("data_points") {
                                    parse_vec3(dp).unwrap_or(Vec3::ZERO)
                                } else {
                                    Vec3::ZERO
                                };

                                match ch {
                                    "rotation" => {
                                        bone_anim.rotations.push(Keyframe {
                                            time,
                                            value: val,
                                            interpolation,
                                        });
                                    }
                                    "position" => {
                                        bone_anim.positions.push(Keyframe {
                                            time,
                                            value: val,
                                            interpolation,
                                        });
                                    }
                                    "scale" => {
                                        bone_anim.scales.push(Keyframe {
                                            time,
                                            value: val,
                                            interpolation,
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        };

                        if let Some(kfs) = b_anim["keyframes"].as_array() {
                            parse_kfs(kfs, None);
                        }

                        if let Some(channels) = b_anim["channels"].as_array() {
                            for ch_obj in channels {
                                let ch_name = ch_obj["name"].as_str().unwrap_or("");
                                if let Some(kfs) = ch_obj["keyframes"].as_array() {
                                    parse_kfs(kfs, Some(ch_name));
                                }
                            }
                        }

                        bone_anim.rotations.sort_by(|a, b| a.time.total_cmp(&b.time));
                        bone_anim.positions.sort_by(|a, b| a.time.total_cmp(&b.time));
                        bone_anim.scales.sort_by(|a, b| a.time.total_cmp(&b.time));

                        // Store under bone UUID, bone key, and lowercase bone name
                        animators.insert(bone_key.clone(), bone_anim.clone());
                        animators.insert(bone_name.to_lowercase(), bone_anim);
                    }
                }

                animations.push(BbAnimation {
                    name: a_name,
                    length,
                    loop_mode,
                    animators,
                });
            }
        }

        Ok(BbModel {
            name,
            resolution,
            elements,
            bones,
            root_bones,
            root_elements,
            textures,
            animations,
        })
    }

    fn load_bedrock_geometry(json: &Value, base_dir: Option<&Path>) -> Result<Self, String> {
        let geos = json["minecraft:geometry"].as_array().ok_or("缺少 minecraft:geometry 数组")?;
        let geo = geos.first().ok_or("minecraft:geometry 为空")?;

        let desc = &geo["description"];
        let name = desc["identifier"].as_str().unwrap_or("Bedrock Model").to_string();
        let tex_w = desc["texture_width"].as_f64().unwrap_or(64.0) as f32;
        let tex_h = desc["texture_height"].as_f64().unwrap_or(64.0) as f32;
        let resolution = Vec2::new(tex_w.max(1.0), tex_h.max(1.0));

        let mut textures = Vec::new();
        // Look for companion texture file
        if let Some(dir) = base_dir {
            for cand_name in ["texture.png", "skin.png"] {
                let cand = dir.join(cand_name);
                if cand.exists() {
                    if let Ok(b) = std::fs::read(&cand) {
                        if let Ok(img) = image::load_from_memory(&b) {
                            let image = img.into_rgba8();
                            let colors = colors_for(&image);
                            textures.push(BbTexture {
                                id: "0".into(),
                                name: cand_name.into(),
                                image,
                                colors,
                            });
                            break;
                        }
                    }
                }
            }
        }
        if textures.is_empty() {
            textures.push(default_texture());
        }

        let mut elements = Vec::new();
        let mut bones = Vec::new();
        let mut bone_name_map = HashMap::new();
        let mut root_bones = Vec::new();

        if let Some(bones_arr) = geo["bones"].as_array() {
            for b in bones_arr {
                let b_name = b["name"].as_str().unwrap_or("bone").to_string();
                let pivot = parse_vec3(&b["pivot"]).unwrap_or(Vec3::ZERO);
                let rotation = parse_vec3(&b["rotation"]).unwrap_or(Vec3::ZERO);

                let b_idx = bones.len();
                bone_name_map.insert(b_name.clone(), b_idx);

                let mut b_elem_indices = Vec::new();

                if let Some(cubes) = b["cubes"].as_array() {
                    for c in cubes {
                        let origin = parse_vec3(&c["origin"]).unwrap_or(Vec3::ZERO);
                        let size = parse_vec3(&c["size"]).unwrap_or(Vec3::ONE);
                        let from = origin;
                        let to = origin + size;
                        let inflate = parse_f32(&c["inflate"]).unwrap_or(0.0);
                        let mirror = c["mirror"].as_bool().unwrap_or(false);

                        let uv_offset = if let Some(arr) = c["uv"].as_array() {
                            Vec2::new(
                                parse_f32(&arr[0]).unwrap_or(0.0),
                                parse_f32(&arr[1]).unwrap_or(0.0),
                            )
                        } else {
                            Vec2::ZERO
                        };

                        let w = size.x;
                        let h = size.y;
                        let d = size.z;
                        let u0 = uv_offset.x;
                        let v0 = uv_offset.y;

                        let box_coords = [
                            [u0 + d + w, v0 + d, u0 + 2.0 * d + w, v0 + d + h],
                            [u0, v0 + d, u0 + d, v0 + d + h],
                            [u0 + d, v0, u0 + d + w, v0 + d],
                            [u0 + d + w, v0, u0 + 2.0 * w + d, v0 + d],
                            [u0 + d, v0 + d, u0 + d + w, v0 + d + h],
                            [u0 + 2.0 * d + w, v0 + d, u0 + 2.0 * d + 2.0 * w, v0 + d + h],
                        ];

                        let mut faces = [None, None, None, None, None, None];
                        for (f_idx, &coords) in box_coords.iter().enumerate() {
                            let mut u1 = coords[0];
                            let mut u2 = coords[2];
                            if mirror {
                                std::mem::swap(&mut u1, &mut u2);
                            }
                            faces[f_idx] = Some(BbFace {
                                uv: [u1, coords[1], u2, coords[3]],
                                texture: 0,
                                rotation: 0,
                            });
                        }

                        let e_idx = elements.len();
                        elements.push(BbElement {
                            uuid: e_idx.to_string(),
                            name: format!("{b_name}_cube_{e_idx}"),
                            from,
                            to,
                            origin: pivot,
                            rotation: Vec3::ZERO,
                            inflate,
                            mirror,
                            faces,
                        });
                        b_elem_indices.push(e_idx);
                    }
                }

                bones.push(BbBone {
                    uuid: b_idx.to_string(),
                    name: b_name,
                    origin: pivot,
                    rotation,
                    parent: None,
                    children_bones: Vec::new(),
                    children_elements: b_elem_indices,
                });
            }

            // Link parent bones
            for (i, b) in bones_arr.iter().enumerate() {
                if let Some(p_name) = b["parent"].as_str() {
                    if let Some(&p_idx) = bone_name_map.get(p_name) {
                        bones[i].parent = Some(p_idx);
                        bones[p_idx].children_bones.push(i);
                    } else {
                        root_bones.push(i);
                    }
                } else {
                    root_bones.push(i);
                }
            }
        }

        Ok(BbModel {
            name,
            resolution,
            elements,
            bones,
            root_bones,
            root_elements: Vec::new(),
            textures,
            animations: Vec::new(),
        })
    }

    pub fn animation_names(&self) -> Vec<String> {
        self.animations.iter().map(|a| a.name.clone()).collect()
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        s: &Settings,
        anim_name: Option<&str>,
        time: f32,
        head_track: Option<Quat>,
        pose: Option<&crate::render::Pose>,
    ) {
        let sit = pose.map(|p| p.sit).unwrap_or(if s.is_sitting() { 1.0 } else { 0.0 });
        let crouch = pose.map(|p| p.crouch).unwrap_or(0.0);

        let mut active_anim = anim_name;
        if sit > 0.5 && (active_anim.is_none() || active_anim == Some("idle")) {
            if let Some(sit_anim) = self.animations.iter().find(|a| {
                let n = a.name.to_lowercase();
                n == "sit" || n == "sitting" || n == "seated"
            }) {
                active_anim = Some(&sit_anim.name);
            }
        }
        let anim = active_anim.and_then(|name| self.animations.iter().find(|a| a.name == name));

        // Evaluate bone transforms
        let mut bone_transforms = vec![Mat4::IDENTITY; self.bones.len()];
        for &root_idx in &self.root_bones {
            self.evaluate_bone_recursive(
                root_idx,
                Mat4::IDENTITY,
                s,
                anim,
                time,
                head_track,
                sit,
                crouch,
                pose,
                &mut bone_transforms,
            );
        }

        let sit_drop = Vec3::new(0., -6.0 * sit - 2.1 * crouch, -1.1 * crouch);
        let anchor = 6.0 * sit;
        let squash_scale = if s.keyboard_bounce {
            pose.map(|p| p.squash).unwrap_or(1.0)
        } else {
            1.0
        };
        let squash = Mat4::from_translation(Vec3::Y * anchor)
            * Mat4::from_scale(Vec3::new(1., squash_scale, 1.))
            * Mat4::from_translation(-Vec3::Y * anchor);

        let global = model_transform(s)
            * squash
            * Mat4::from_translation(sit_drop)
            * Mat4::from_rotation_y(std::f32::consts::PI);

        // Render elements attached to bones
        for (bone_idx, bone) in self.bones.iter().enumerate() {
            let b_mat = bone_transforms[bone_idx];
            for &e_idx in &bone.children_elements {
                let elem = &self.elements[e_idx];
                self.draw_element(frame, s, global, b_mat, elem);
            }
        }

        // Render root elements
        for &e_idx in &self.root_elements {
            let elem = &self.elements[e_idx];
            self.draw_element(frame, s, global, Mat4::IDENTITY, elem);
        }

        // Seated keyboard in front of character
        if sit > 0.4 {
            frame.draw_keyboard(s, sit);
        }
    }

    fn evaluate_bone_recursive(
        &self,
        bone_idx: usize,
        parent_transform: Mat4,
        s: &Settings,
        anim: Option<&BbAnimation>,
        time: f32,
        head_track: Option<Quat>,
        sit: f32,
        crouch: f32,
        pose: Option<&crate::render::Pose>,
        bone_transforms: &mut [Mat4],
    ) {
        let bone = &self.bones[bone_idx];

        let mut anim_data = if let Some(a) = anim {
            let t = if a.length > 0.0 {
                match a.loop_mode {
                    BbLoopMode::Loop => time % a.length,
                    BbLoopMode::Once | BbLoopMode::Hold => time.min(a.length),
                }
            } else {
                0.0
            };

            let animator = a
                .animators
                .get(&bone.uuid)
                .or_else(|| a.animators.get(&bone.name.to_lowercase()));

            if let Some(animator) = animator {
                (
                    sample_vec3(&animator.rotations, t, Vec3::ZERO),
                    sample_vec3(&animator.positions, t, Vec3::ZERO),
                    sample_vec3(&animator.scales, t, Vec3::ONE),
                )
            } else {
                (Vec3::ZERO, Vec3::ZERO, Vec3::ONE)
            }
        } else {
            (Vec3::ZERO, Vec3::ZERO, Vec3::ONE)
        };

        // If bone is eyelids and not actively animated, allow natural periodic blinking (every 3.5s)
        if bone.name.to_lowercase().contains("eyelid") && anim_data.1 == Vec3::ZERO {
            if let Some(blink) = self.animations.iter().find(|a| a.name.to_lowercase() == "blink") {
                let cycle = time % 3.5;
                if cycle < blink.length && blink.length > 0.0 {
                    let animator = blink
                        .animators
                        .get(&bone.uuid)
                        .or_else(|| blink.animators.get(&bone.name.to_lowercase()));
                    if let Some(animator) = animator {
                        anim_data = (
                            sample_vec3(&animator.rotations, cycle, Vec3::ZERO),
                            sample_vec3(&animator.positions, cycle, Vec3::ZERO),
                            sample_vec3(&animator.scales, cycle, Vec3::ONE),
                        );
                    }
                }
            }
        }

        let (anim_rot, anim_pos, anim_scale) = anim_data;

        let is_head = bone.name.to_lowercase().contains("head");
        let extra_rot = if is_head {
            if let Some(track) = head_track {
                let (rx, ry, rz) = track.to_euler(EulerRot::XYZ);
                // Invert rx (pitch) and rz so mouse follow in Blockbench space correctly mirrors RotY(PI)
                Quat::from_euler(EulerRot::XYZ, -rx, ry, -rz)
            } else if crouch > 0.001 {
                // When not tracking mouse, compensate for torso forward lean by tilting head up
                Quat::from_rotation_x(0.25 * crouch)
            } else {
                Quat::IDENTITY
            }
        } else {
            Quat::IDENTITY
        };

        let bone_lower = bone.name.to_lowercase();
        let is_left_leg = bone_lower == "leftleg"
            || bone_lower == "left_leg"
            || bone_lower == "leg_left"
            || bone_lower == "leg1"
            || bone_lower == "left_leg_bone";
        let is_right_leg = bone_lower == "rightleg"
            || bone_lower == "right_leg"
            || bone_lower == "leg_right"
            || bone_lower == "leg0"
            || bone_lower == "right_leg_bone";
        let is_left_arm = bone_lower == "leftarm"
            || bone_lower == "left_arm"
            || bone_lower == "arm_left"
            || bone_lower == "arm1";
        let is_right_arm = bone_lower == "rightarm"
            || bone_lower == "right_arm"
            || bone_lower == "arm_right"
            || bone_lower == "arm0";
        let is_body = bone_lower == "body" || bone_lower == "torso";
        let is_tail = bone_lower == "tail"
            || bone_lower == "cattail"
            || bone_lower == "cat_tail";

        let mut parent_is_body = false;
        let mut cur_p = bone.parent;
        while let Some(p_idx) = cur_p {
            let p_name = self.bones[p_idx].name.to_lowercase();
            if p_name == "body" || p_name == "torso" {
                parent_is_body = true;
                break;
            }
            cur_p = self.bones[p_idx].parent;
        }

        let procedural_rot = if (is_left_leg || is_right_leg) && (anim_rot.length_squared() < 0.001 || anim.map(|a| !a.name.to_lowercase().contains("sit")).unwrap_or(true)) {
            // Legs in world space rotate +0.10 * crouch around X, which maps to -0.10 * crouch in Blockbench space
            let leg_pitch = std::f32::consts::PI * 0.48 * sit - 0.10 * crouch;
            if is_left_leg {
                Quat::from_euler(
                    EulerRot::XYZ,
                    leg_pitch,
                    -0.08 * sit,
                    0.03 * sit,
                )
            } else {
                Quat::from_euler(
                    EulerRot::XYZ,
                    leg_pitch,
                    0.08 * sit,
                    -0.03 * sit,
                )
            }
        } else if is_body && anim_rot.length_squared() < 0.001 {
            if crouch > 0.001 {
                // Torso leans forward by +0.34 * crouch in world space (-0.34 in Blockbench space)
                Quat::from_rotation_x(-0.34 * crouch)
            } else {
                Quat::IDENTITY
            }
        } else if (is_left_arm || is_right_arm) && anim_rot.length_squared() < 0.001 {
            let crouch_rot = if !parent_is_body && crouch > 0.001 {
                Quat::from_rotation_x(-0.34 * crouch)
            } else {
                Quat::IDENTITY
            };
            let side = if is_left_arm { 0 } else { 1 };
            if let Some(p) = pose {
                // p.arms[side] contains the live arm rotation in Desktop Pet coordinate space (forward is +Z).
                // In Blockbench space (North is -Z, rotated by 180° around Y):
                // Forward angle is inverted (rx -> -rx), inward angle is inverted (ry -> -ry).
                let (rx, ry, rz) = p.arms[side].to_euler(EulerRot::XYZ);
                crouch_rot * Quat::from_euler(EulerRot::XYZ, -rx, ry, -rz)
            } else if sit > 0.001 {
                if is_left_arm {
                    crouch_rot * Quat::from_euler(EulerRot::XYZ, std::f32::consts::PI * 0.26 * sit, 0.10 * sit, -0.05 * sit)
                } else {
                    crouch_rot * Quat::from_euler(EulerRot::XYZ, std::f32::consts::PI * 0.26 * sit, -0.10 * sit, 0.05 * sit)
                }
            } else {
                crouch_rot
            }
        } else if is_tail && sit > 0.001 && anim_rot.length_squared() < 0.001 {
            // When sitting, curl the base tail upwards and slightly sideways to avoid clipping into the floor
            Quat::from_euler(EulerRot::XYZ, (-35.0f32).to_radians() * sit, 0.20 * sit, 0.05 * sit)
        } else {
            Quat::IDENTITY
        };

        // User custom pose when s.posture == "custom"
        let custom_pose_rot = if s.posture == "custom" {
            let p = s.active_custom_pose();
            let row = if is_head {
                Some(0)
            } else if bone_lower == "body" || bone_lower == "torso" {
                Some(1)
            } else if is_left_arm {
                Some(2)
            } else if is_right_arm {
                Some(3)
            } else if is_left_leg {
                Some(4)
            } else if is_right_leg {
                Some(5)
            } else {
                None
            };
            if let Some(r) = row {
                let degs = p.row(r);
                Quat::from_euler(EulerRot::XYZ, -degs[0].to_radians(), degs[1].to_radians(), -degs[2].to_radians())
            } else {
                Quat::IDENTITY
            }
        } else {
            Quat::IDENTITY
        };

        let total_rot = Quat::from_euler(
            EulerRot::XYZ,
            (bone.rotation.x + anim_rot.x).to_radians(),
            (bone.rotation.y + anim_rot.y).to_radians(),
            (bone.rotation.z + anim_rot.z).to_radians(),
        ) * extra_rot * procedural_rot * custom_pose_rot;

        let mut pivot = bone.origin;
        let mut crouch_pos_offset = Vec3::ZERO;
        if crouch > 0.001 {
            if is_body {
                // Body leans forward around waist (Y = 12.0)
                if pivot.y > 14.0 {
                    pivot.y = 12.0;
                }
            } else if !parent_is_body {
                if is_head {
                    // Head origin is at Y = 24. Rotating around waist Y = 12 by -0.34 rad in Blockbench space:
                    // dY = 12 * cos(-0.34) - 12 = -0.69
                    // dZ = 12 * sin(-0.34) = -4.00
                    crouch_pos_offset = Vec3::new(0., -0.69 * crouch, -4.00 * crouch);
                } else if is_left_arm || is_right_arm {
                    // Arm origin is at Y = 22. Rotating around waist Y = 12 by -0.34 rad in Blockbench space:
                    // dY = 10 * cos(-0.34) - 10 = -0.57
                    // dZ = 10 * sin(-0.34) = -3.33
                    crouch_pos_offset = Vec3::new(0., -0.57 * crouch, -3.33 * crouch);
                }
            }
        }

        let local = Mat4::from_translation(pivot + anim_pos + crouch_pos_offset)
            * Mat4::from_quat(total_rot)
            * Mat4::from_scale(anim_scale)
            * Mat4::from_translation(-pivot);

        let world = parent_transform * local;
        bone_transforms[bone_idx] = world;

        for &ch_idx in &bone.children_bones {
            self.evaluate_bone_recursive(
                ch_idx,
                world,
                s,
                anim,
                time,
                head_track,
                sit,
                crouch,
                pose,
                bone_transforms,
            );
        }
    }

    fn draw_element(
        &self,
        frame: &mut Frame,
        _s: &Settings,
        global: Mat4,
        bone_mat: Mat4,
        elem: &BbElement,
    ) {
        let elem_rot = if elem.rotation.length_squared() > 0.0001 {
            Mat4::from_translation(elem.origin)
                * Mat4::from_euler(
                    EulerRot::XYZ,
                    elem.rotation.x.to_radians(),
                    elem.rotation.y.to_radians(),
                    elem.rotation.z.to_radians(),
                )
                * Mat4::from_translation(-elem.origin)
        } else {
            Mat4::IDENTITY
        };

        let m = global * bone_mat * elem_rot;

        let min = elem.from - Vec3::splat(elem.inflate);
        let max = elem.to + Vec3::splat(elem.inflate);

        // 6 faces in Blockbench:
        // 0: East(+X), 1: West(-X), 2: Up(+Y), 3: Down(-Y), 4: South(+Z), 5: North(-Z)
        // Order from Blockbench Three.js BoxGeometry:
        let face_vertices = [
            // East (+X)
            [
                Vec3::new(max.x, max.y, max.z),
                Vec3::new(max.x, max.y, min.z),
                Vec3::new(max.x, min.y, max.z),
                Vec3::new(max.x, min.y, min.z),
            ],
            // West (-X)
            [
                Vec3::new(min.x, max.y, min.z),
                Vec3::new(min.x, max.y, max.z),
                Vec3::new(min.x, min.y, min.z),
                Vec3::new(min.x, min.y, max.z),
            ],
            // Up (+Y)
            [
                Vec3::new(min.x, max.y, min.z),
                Vec3::new(max.x, max.y, min.z),
                Vec3::new(min.x, max.y, max.z),
                Vec3::new(max.x, max.y, max.z),
            ],
            // Down (-Y)
            [
                Vec3::new(min.x, min.y, max.z),
                Vec3::new(max.x, min.y, max.z),
                Vec3::new(min.x, min.y, min.z),
                Vec3::new(max.x, min.y, min.z),
            ],
            // South (+Z)
            [
                Vec3::new(min.x, max.y, max.z),
                Vec3::new(max.x, max.y, max.z),
                Vec3::new(min.x, min.y, max.z),
                Vec3::new(max.x, min.y, max.z),
            ],
            // North (-Z)
            [
                Vec3::new(max.x, max.y, min.z),
                Vec3::new(min.x, max.y, min.z),
                Vec3::new(max.x, min.y, min.z),
                Vec3::new(min.x, min.y, min.z),
            ],
        ];

        let fw = frame.width as f32;
        let fh = frame.height as f32;

        for (f_idx, &quad) in face_vertices.iter().enumerate() {
            let Some(face) = &elem.faces[f_idx] else {
                continue;
            };

            let tex_idx = face.texture.min(self.textures.len().saturating_sub(1));
            let tex = &self.textures[tex_idx];

            let mut pos = [Vec3::ZERO; 4];
            for i in 0..4 {
                let world_v = m.transform_point3(quad[i]);
                pos[i] = project(world_v, fw, fh);
            }

            // Backface culling: front-facing triangles have area < 0.0
            let area = crate::render::edge(pos[0], pos[2], pos[1]);
            if area >= 0.0 {
                continue;
            }

            // Scale UV coordinates to match actual texture resolution
            let scale_u = tex.image.width() as f32 / self.resolution.x;
            let scale_v = tex.image.height() as f32 / self.resolution.y;

            let u1 = face.uv[0] * scale_u;
            let v1 = face.uv[1] * scale_v;
            let u2 = face.uv[2] * scale_u;
            let v2 = face.uv[3] * scale_v;

            let mut uvs = [
                Vec2::new(u1, v1),
                Vec2::new(u2, v1),
                Vec2::new(u1, v2),
                Vec2::new(u2, v2),
            ];

            // Face texture rotation (0, 90, 180, 270)
            match face.rotation {
                90 => uvs = [uvs[2], uvs[0], uvs[3], uvs[1]],
                180 => uvs = [uvs[3], uvs[2], uvs[1], uvs[0]],
                270 => uvs = [uvs[1], uvs[3], uvs[0], uvs[2]],
                _ => {}
            }

            // Triangle 1: [0, 2, 1]
            frame.draw_triangle(
                [pos[0], pos[2], pos[1]],
                [uvs[0], uvs[2], uvs[1]],
                f_idx,
                &tex.image,
                &tex.colors,
            );

            // Triangle 2: [2, 3, 1]
            frame.draw_triangle(
                [pos[2], pos[3], pos[1]],
                [uvs[2], uvs[3], uvs[1]],
                f_idx,
                &tex.image,
                &tex.colors,
            );
        }
    }
}

fn sample_vec3(keyframes: &[Keyframe<Vec3>], t: f32, default: Vec3) -> Vec3 {
    if keyframes.is_empty() {
        return default;
    }
    if keyframes.len() == 1 || t <= keyframes[0].time {
        return keyframes[0].value;
    }
    if t >= keyframes.last().unwrap().time {
        return keyframes.last().unwrap().value;
    }

    for i in 0..keyframes.len() - 1 {
        let k0 = &keyframes[i];
        let k1 = &keyframes[i + 1];
        if t >= k0.time && t <= k1.time {
            let dt = k1.time - k0.time;
            if dt <= 0.00001 {
                return k0.value;
            }
            let alpha = (t - k0.time) / dt;
            return match k0.interpolation {
                Interpolation::Step => k0.value,
                Interpolation::Smooth => {
                    let s = alpha * alpha * (3.0 - 2.0 * alpha);
                    k0.value.lerp(k1.value, s)
                }
                Interpolation::Linear => k0.value.lerp(k1.value, alpha),
            };
        }
    }

    keyframes.last().unwrap().value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_bbmodel() {
        let json = r#"{
            "meta": { "format_version": "4.0" },
            "name": "test_box",
            "resolution": { "width": 64, "height": 64 },
            "elements": [
                {
                    "name": "head",
                    "from": [-4, 0, -4],
                    "to": [4, 8, 4],
                    "origin": [0, 0, 0],
                    "faces": {
                        "north": { "uv": [0, 0, 8, 8], "texture": 0 },
                        "south": { "uv": [8, 0, 16, 8], "texture": 0 }
                    }
                }
            ],
            "outliner": [
                {
                    "name": "head_bone",
                    "origin": [0, 0, 0],
                    "children": ["head"]
                }
            ]
        }"#;

        let model = BbModel::load(json.as_bytes(), None).unwrap();
        assert_eq!(model.name, "test_box");
        assert_eq!(model.elements.len(), 1);
        assert_eq!(model.elements[0].name, "head");
        assert_eq!(model.bones.len(), 1);
        assert_eq!(model.bones[0].name, "head_bone");
        assert_eq!(model.bones[0].children_elements, vec![0]);
    }

    #[test]
    fn animation_keyframe_sampling() {
        let kfs = vec![
            Keyframe {
                time: 0.0,
                value: Vec3::new(0.0, 0.0, 0.0),
                interpolation: Interpolation::Linear,
            },
            Keyframe {
                time: 1.0,
                value: Vec3::new(10.0, 20.0, 30.0),
                interpolation: Interpolation::Linear,
            },
        ];

        let v0 = sample_vec3(&kfs, 0.0, Vec3::ZERO);
        assert_eq!(v0, Vec3::ZERO);

        let v_half = sample_vec3(&kfs, 0.5, Vec3::ZERO);
        assert!((v_half.x - 5.0).abs() < 0.001);
        assert!((v_half.y - 10.0).abs() < 0.001);
        assert!((v_half.z - 15.0).abs() < 0.001);

        let v1 = sample_vec3(&kfs, 1.0, Vec3::ZERO);
        assert_eq!(v1, Vec3::new(10.0, 20.0, 30.0));
    }

    #[test]
    fn renders_bbmodel_to_frame() {
        let json = r#"{
            "meta": { "format_version": "4.0" },
            "name": "cube",
            "resolution": { "width": 16, "height": 16 },
            "elements": [
                {
                    "name": "body",
                    "from": [-4, 8, -4],
                    "to": [4, 16, 4],
                    "origin": [0, 12, 0],
                    "box_uv": true,
                    "uv_offset": [0, 0]
                }
            ],
            "outliner": ["body"]
        }"#;

        let model = BbModel::load(json.as_bytes(), None).unwrap();
        let mut frame = Frame::new(360, 420);
        let s = Settings::default();
        model.draw(&mut frame, &s, None, 0.0, None, None);

        let visible_pixels = frame.rgba().chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(visible_pixels > 50, "模型应该被渲染并产生不透明像素，当前像素数: {visible_pixels}");
    }

    #[test]
    fn parse_bedrock_geometry_format() {
        let json = r#"{
            "format_version": "1.12.0",
            "minecraft:geometry": [
                {
                    "description": {
                        "identifier": "geometry.creeper",
                        "texture_width": 64,
                        "texture_height": 32
                    },
                    "bones": [
                        {
                            "name": "body",
                            "pivot": [0, 12, 0],
                            "cubes": [
                                {
                                    "origin": [-4, 6, -2],
                                    "size": [8, 12, 4],
                                    "uv": [16, 16]
                                }
                            ]
                        },
                        {
                            "name": "head",
                            "parent": "body",
                            "pivot": [0, 18, 0],
                            "cubes": [
                                {
                                    "origin": [-4, 18, -4],
                                    "size": [8, 8, 8],
                                    "uv": [0, 0]
                                }
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let model = BbModel::load(json.as_bytes(), None).unwrap();
        assert_eq!(model.name, "geometry.creeper");
        assert_eq!(model.resolution, Vec2::new(64.0, 32.0));
        assert_eq!(model.bones.len(), 2);
        assert_eq!(model.elements.len(), 2);
        assert_eq!(model.root_bones.len(), 1);
        assert_eq!(model.bones[0].name, "body");
        assert_eq!(model.bones[1].name, "head");
        assert_eq!(model.bones[1].parent, Some(0));
    }

    #[test]
    fn animation_with_head_track_and_loop() {
        let json = r#"{
            "meta": { "format_version": "4.0" },
            "name": "animated_pet",
            "resolution": { "width": 16, "height": 16 },
            "elements": [
                {
                    "name": "head",
                    "from": [-4, 16, -4],
                    "to": [4, 24, 4],
                    "origin": [0, 16, 0],
                    "box_uv": true,
                    "uv_offset": [0, 0]
                }
            ],
            "outliner": [
                {
                    "name": "head",
                    "origin": [0, 16, 0],
                    "children": ["head"]
                }
            ],
            "animations": [
                {
                    "name": "nod",
                    "length": 2.0,
                    "loop": "loop",
                    "animators": {
                        "head": {
                            "name": "head",
                            "keyframes": [
                                {
                                    "channel": "rotation",
                                    "time": 0.0,
                                    "data_points": [{ "x": 0, "y": 0, "z": 0 }],
                                    "interpolation": "linear"
                                },
                                {
                                    "channel": "rotation",
                                    "time": 1.0,
                                    "data_points": [{ "x": 20, "y": 0, "z": 0 }],
                                    "interpolation": "linear"
                                }
                            ]
                        }
                    }
                }
            ]
        }"#;

        let model = BbModel::load(json.as_bytes(), None).unwrap();
        assert_eq!(model.animations.len(), 1);
        assert_eq!(model.animations[0].name, "nod");
        assert_eq!(model.animations[0].length, 2.0);

        let mut frame = Frame::new(360, 420);
        let s = Settings::default();
        let head_gaze = Quat::from_rotation_y(0.2);
        model.draw(&mut frame, &s, Some("nod"), 0.5, Some(head_gaze), None);

        let count = frame.rgba().chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(count > 50);
    }

    #[test]
    fn load_example_duck_asset() {
        let path = std::path::Path::new("../assets/example_duck.bbmodel");
        if path.exists() {
            let model = BbModel::load_file(path).unwrap();
            assert_eq!(model.name, "cute_duck");
            assert_eq!(model.elements.len(), 7);
            assert_eq!(model.bones.len(), 6);
            assert_eq!(model.animations.len(), 2);
            assert_eq!(model.animation_names(), vec!["idle", "flap"]);

            let mut frame = Frame::new(360, 420);
            let s = Settings::default();
            model.draw(&mut frame, &s, Some("idle"), 0.5, None, None);
            let count = frame.rgba().chunks_exact(4).filter(|p| p[3] > 0).count();
            assert!(count > 100);
        }
    }

    #[test]
    fn render_test_frames() {
        let path = std::path::Path::new(r"c:\Users\Rui_Shu\Desktop\model.bbmodel");
        if !path.exists() {
            return;
        }
        let model = BbModel::load_file(path).unwrap();

        // Standing frame
        let mut frame_stand = Frame::new(450, 525);
        let s_stand = Settings::default();
        let pose_stand = crate::render::Pose::new(&s_stand);
        model.draw(&mut frame_stand, &s_stand, None, 0.0, None, Some(&pose_stand));
        let raw_stand = frame_stand.rgba();
        let img_stand = image::RgbaImage::from_raw(450, 525, raw_stand).unwrap();
        let _ = img_stand.save(r"C:\Users\Rui_Shu\.gemini\antigravity\brain\cfe8a929-ab05-47cd-85ca-6cd72f32f69d\scratch\rendered_user_standing.png");

        // Sitting frame
        let mut frame_sit = Frame::new(450, 525);
        let mut s_sit = Settings::default();
        s_sit.posture = "sitting".into();
        let mut pose_sit = crate::render::Pose::new(&s_sit);
        let skin = crate::render::Skin::load(crate::render::DEFAULT_SKIN).unwrap();
        for _ in 0..60 {
            pose_sit.update(&s_sit, &skin, glam::Vec2::ZERO, 450.0, 525.0, false, 1.0, 0.016);
        }
        model.draw(&mut frame_sit, &s_sit, None, 0.0, None, Some(&pose_sit));
        let raw_sit = frame_sit.rgba();
        let img_sit = image::RgbaImage::from_raw(450, 525, raw_sit).unwrap();
        let _ = img_sit.save(r"C:\Users\Rui_Shu\.gemini\antigravity\brain\cfe8a929-ab05-47cd-85ca-6cd72f32f69d\scratch\rendered_user_sitting.png");
    }

    #[test]
    fn head_track_pitch_applied() {
        let json = r#"{
            "meta": { "model_format": "free" },
            "resolution": { "width": 16, "height": 16 },
            "elements": [
                {
                    "name": "head",
                    "from": [-4, 16, -4],
                    "to": [4, 24, 4],
                    "origin": [0, 16, 0],
                    "box_uv": true,
                    "uv_offset": [0, 0]
                }
            ],
            "outliner": [
                {
                    "name": "head",
                    "origin": [0, 16, 0],
                    "children": ["head"]
                }
            ]
        }"#;
        let model = BbModel::load(json.as_bytes(), None).unwrap();
        let s = Settings::default();

        // Pitch up in world space (pitch angle rx < 0)
        let head_pitch = Quat::from_rotation_x(-0.3);
        let mut frame = Frame::new(100, 100);
        model.draw(&mut frame, &s, None, 0.0, Some(head_pitch), None);
        let count = frame.rgba().chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(count > 0);
    }

    #[test]
    fn inspect_arm_typing_coordinates() {
        let s = Settings::default();
        let mut p = crate::render::Pose::new(&s);
        let skin = crate::render::Skin::load(crate::render::DEFAULT_SKIN).unwrap();
        // Update pose in sitting posture
        let mut s_sit = s.clone();
        s_sit.posture = "sitting".into();
        p.update(&s_sit, &skin, glam::Vec2::ZERO, 100., 100., false, 1.0, 0.016);

        for side in 0..2 {
            let q = p.arms[side];
            let (rx, ry, rz) = q.to_euler(EulerRot::XYZ);
            let hand_player = q * Vec3::new(0., -10., 0.);

            let new_bb_q = Quat::from_euler(EulerRot::XYZ, -rx, ry, -rz);
            let new_bb_world = Mat4::from_rotation_y(std::f32::consts::PI).transform_vector3(new_bb_q * Vec3::new(0., -10., 0.));

            println!("SIDE {}: player hand vec: {:?}, new_bb_world: {:?}", side, hand_player, new_bb_world);
            assert!((new_bb_world - hand_player).length() < 0.01);
        }
    }

    #[test]
    fn blockbench_typing_squash_and_toggle() {
        let mut s = Settings::default();
        s.model_mode = "blockbench".into();
        s.keyboard_bounce = true;
        s.keyboard_enabled = true;
        s.typing_squash = 10.0;

        let mut p = crate::render::Pose::new(&s);
        let skin = crate::render::Skin::load(crate::render::DEFAULT_SKIN).unwrap();
        // Press a key at t = 1.0s
        p.key(65, true, 1.0, &s);
        // At t = 1.08s (peak of pulse), update pose
        p.update(&s, &skin, glam::Vec2::ZERO, 100., 100., false, 1.08, 0.08);
        assert!((p.squash - 0.90).abs() < 1e-4);

        // When switch is turned off, squash is disabled
        s.keyboard_bounce = false;
        p.update(&s, &skin, glam::Vec2::ZERO, 100., 100., false, 1.08, 0.08);
        assert_eq!(p.squash, 1.0);
    }

    #[test]
    fn crouch_alignment_with_player_model() {
        let json = r#"{
            "meta": { "model_format": "free" },
            "resolution": { "width": 64, "height": 64 },
            "elements": [
                {
                    "name": "body",
                    "from": [-4, 12, -2],
                    "to": [4, 24, 2]
                },
                {
                    "name": "left_leg",
                    "from": [-4, 0, -2],
                    "to": [0, 12, 2]
                }
            ],
            "outliner": [
                {
                    "name": "Body",
                    "origin": [0, 24, 0],
                    "children": ["body"]
                },
                {
                    "name": "LeftLeg",
                    "origin": [-1.9, 12, 0],
                    "children": ["left_leg"]
                }
            ]
        }"#;
        let model = BbModel::load(json.as_bytes(), None).unwrap();
        let mut s = Settings::default();
        s.posture = "standing".into();
        s.model_rotation = [0.0, -68.0, 0.0];

        let skin = crate::render::Skin::load(crate::render::DEFAULT_SKIN).unwrap();
        let mut p = crate::render::Pose::new(&s);
        p.update(&s, &skin, glam::Vec2::ZERO, 100., 100., true, 1.0, 0.016); // shift = true -> crouch = 1.0
        assert_eq!(p.crouch, 1.0);

        let mut bone_transforms = vec![Mat4::IDENTITY; model.bones.len()];
        let sit = 0.0;
        let crouch = 1.0;
        for &root_idx in &model.root_bones {
            model.evaluate_bone_recursive(
                root_idx,
                Mat4::IDENTITY,
                &s,
                None,
                0.0,
                None,
                sit,
                crouch,
                Some(&p),
                &mut bone_transforms,
            );
        }

        let sit_drop = Vec3::new(0., -6.0 * sit - 2.1 * crouch, -1.1 * crouch);
        let global = crate::render::model_transform(&s)
            * Mat4::from_translation(sit_drop)
            * Mat4::from_rotation_y(std::f32::consts::PI);

        // Check LeftLeg bone transform
        let leg_idx = model.bones.iter().position(|b| b.name == "LeftLeg").unwrap();
        let leg_world = global * bone_transforms[leg_idx];
        let leg_bone = &model.bones[leg_idx];
        let hip_pos = leg_world.transform_point3(leg_bone.origin);
        let foot_pos = leg_world.transform_point3(leg_bone.origin - Vec3::new(0.0, 12.0, 0.0));

        println!("LeftLeg hip pos = {:?}, foot pos = {:?}", hip_pos, foot_pos);

        // Hip Y should drop from 12 to 9.9
        assert!((hip_pos.y - 9.90).abs() < 0.05);

        // Check Body bone transform
        let body_idx = model.bones.iter().position(|b| b.name == "Body").unwrap();
        let body_world = global * bone_transforms[body_idx];
        let waist_pos = body_world.transform_point3(Vec3::new(0.0, 12.0, 0.0));
        let neck_pos = body_world.transform_point3(Vec3::new(0.0, 24.0, 0.0));
        println!("Body waist pos = {:?}, neck pos = {:?}", waist_pos, neck_pos);
        assert!((waist_pos.y - 9.90).abs() < 0.05);
        assert!((neck_pos.y - 21.21).abs() < 0.10);
    }
}
