//! Original renderer exposed through a main-thread-only C ABI for AppKit.
#![allow(dead_code)]
mod config;
mod keyboard;
mod render;
use config::{Settings, Store};
use glam::Vec2;
use render::{Frame, Pose, Skin};
use std::{ffi::{c_char, CStr, CString}, path::PathBuf, ptr};

pub struct PetCore {
    settings: Settings,
    store: Store,
    skin: Skin,
    pose: Pose,
    frame: Frame,
    message: CString,
    json: CString,
}
impl PetCore {
    fn error(&mut self, e: String) -> bool {
        self.message = CString::new(e.replace('\0', "")).unwrap();
        false
    }
    fn update_settings(&mut self, s: Settings) {
        // Match the Windows command handlers: persistence is not an input reset.
        if self.settings.posture != s.posture
            || self.settings.custom_base != s.custom_base
            || self.settings.typing_mode != s.typing_mode
            || self.settings.keyboard_enabled != s.keyboard_enabled {
            self.pose.reset_keys();
        }
        if self.settings.main_hand != s.main_hand
            || self.settings.posture != s.posture
            || self.settings.custom_base != s.custom_base {
            self.pose.reset_swing();
        }
        self.settings = s;
    }
    fn save(&mut self) -> bool {
        match self.store.save(&self.settings) {
            Ok(()) => { self.message = CString::default(); true }
            Err(e) => self.error(e),
        }
    }
}
unsafe fn string(p: *const c_char) -> Option<String> {
    if p.is_null() { None } else { CStr::from_ptr(p).to_str().ok().map(str::to_owned) }
}
#[no_mangle]
pub unsafe extern "C" fn pet_create(dir: *const c_char) -> *mut PetCore {
    let Some(dir) = string(dir) else { return ptr::null_mut(); };
    let mut store = Store::new(PathBuf::from(dir));
    let settings = store.load();
    if store.read_error.is_some() { return ptr::null_mut(); }
    let bytes = std::fs::read(&settings.skin_path).unwrap_or_else(|_| render::DEFAULT_SKIN.to_vec());
    let skin = Skin::load(&bytes).unwrap_or_else(|_| Skin::load(render::DEFAULT_SKIN).unwrap());
    let pose = Pose::new(&settings);
    Box::into_raw(Box::new(PetCore {settings, store, skin, pose, frame: Frame::new(450, 525), message: CString::default(), json: CString::default()}))
}
#[no_mangle]
pub unsafe extern "C" fn pet_destroy(core: *mut PetCore) {
    if !core.is_null() { drop(Box::from_raw(core)); }
}
#[no_mangle]
pub unsafe extern "C" fn pet_settings(core: *mut PetCore) -> *const c_char {
    let Some(c) = core.as_mut() else { return ptr::null(); };
    c.json = CString::new(serde_json::to_string(&c.settings).unwrap()).unwrap();
    c.json.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn pet_error(core: *mut PetCore) -> *const c_char {
    core.as_ref().map_or(ptr::null(), |c| c.message.as_ptr())
}
#[no_mangle]
pub unsafe extern "C" fn pet_preview(core: *mut PetCore, json: *const c_char) -> bool {
    let (Some(c), Some(json)) = (core.as_mut(), string(json)) else { return false; };
    match serde_json::from_str::<Settings>(&json) {
        Ok(mut s) => { s.clean(); c.update_settings(s); true },
        Err(e) => c.error(e.to_string()),
    }
}
#[no_mangle]
pub unsafe extern "C" fn pet_apply(core: *mut PetCore, json: *const c_char) -> bool {
    let (Some(c), Some(json)) = (core.as_mut(), string(json)) else { return false; };
    match serde_json::from_str::<Settings>(&json) {
        Ok(mut s) => {
            s.clean();
            let old = c.settings.clone();
            c.update_settings(s);
            if !c.save() { c.settings = old; return false; }
            true
        },
        Err(e) => c.error(e.to_string()),
    }
}
#[no_mangle]
pub unsafe extern "C" fn pet_import(core: *mut PetCore, path: *const c_char) -> bool {
    let (Some(c), Some(path)) = (core.as_mut(), string(path)) else { return false; };
    let result = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| Skin::load(&bytes).map(|skin| (bytes, skin)));
    match result {
        Ok((bytes, skin)) => {
            let dest = c.store.dir.join("skin.png");
            if let Err(e) = config::atomic(&dest, &bytes) { return c.error(e); }
            c.settings.skin_path = dest.to_string_lossy().into_owned();
            c.settings.skin_name = PathBuf::from(path).file_name().unwrap_or_default().to_string_lossy().into_owned();
            if skin.legacy { c.settings.arm_model = "classic".into(); }
            c.skin = skin;
            c.save()
        },
        Err(e) => c.error(e),
    }
}
#[no_mangle]
pub unsafe extern "C" fn pet_reset(core: *mut PetCore) -> bool {
    let Some(c) = core.as_mut() else { return false; };
    c.settings = Settings::first_run_defaults();
    c.skin = Skin::load(render::DEFAULT_SKIN).unwrap();
    c.pose = Pose::new(&c.settings);
    c.save()
}
#[no_mangle]
pub unsafe extern "C" fn pet_key(core: *mut PetCore, key: u32, down: bool, now: f32) {
    if let Some(c) = core.as_mut() { c.pose.key(key, down, now, &c.settings); }
}
#[no_mangle]
pub unsafe extern "C" fn pet_clear_keys(core: *mut PetCore) {
    if let Some(c) = core.as_mut() { c.pose.reset_keys(); }
}
#[no_mangle]
pub unsafe extern "C" fn pet_click(core: *mut PetCore, now: f32) {
    if let Some(c) = core.as_mut() { c.pose.click(now, c.settings.main_hand == "left"); }
}
#[no_mangle]
pub unsafe extern "C" fn pet_render(core: *mut PetCore, x: f32, y: f32, width: f32, height: f32, shift: bool, now: f32, dt: f32) -> *const u8 {
    let Some(c) = core.as_mut() else { return ptr::null(); };
    c.pose.update(&c.settings, &c.skin, Vec2::new(x, y), width, height, shift, now, dt);
    c.frame.draw(&c.settings, &c.skin, &c.pose);
    c.frame.pixels.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn pet_hit(core: *mut PetCore, x: f32, y: f32) -> bool {
    let Some(c) = core.as_ref() else { return false; };
    if x < 0. || y < 0. || x >= 1. || y >= 1. { return false; }
    c.frame.pixels[((y * 525.) as usize * 450 + (x * 450.) as usize) * 4 + 3] != 0
}
/// 当前闭眼程度（0 = 睁开，1 = 闭合）。宿主在静止时不重绘，
/// 用它判断是否需要在眨眼期间额外出帧。
#[no_mangle]
pub unsafe extern "C" fn pet_blink_at(core: *mut PetCore, now: f32) -> f32 {
    core.as_ref().map_or(0., |c| c.pose.blink_at(now))
}

#[cfg(test)]
mod ffi_tests {
    use super::*;
    #[test]
    fn macos_save_and_position_changes_do_not_interrupt_original_animation() {
        let dir = std::env::temp_dir().join(format!("pet-macos-motion-{}",std::process::id()));
        let path = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let core = pet_create(path.as_ptr());
            for posture in ["standing", "sitting", "custom"] {
                for hand in ["left", "right"] {
                    for mode in ["precise", "alternating"] {
                        let mut s = Settings::first_run_defaults();
                        s.posture = posture.into(); s.custom_base = "sitting".into();
                        s.main_hand = hand.into(); s.typing_mode = mode.into(); s.track_mouse = false;
                        s.hyp_quick_chat = true;
                        let json = CString::new(serde_json::to_string(&s).unwrap()).unwrap();
                        assert!(pet_apply(core, json.as_ptr()));
                        (*core).pose = Pose::new(&s);
                        let mut reference = Pose::new(&s);
                        let skin = Skin::load(render::DEFAULT_SKIN).unwrap();
                        for frame in 0..180 {
                            let now = frame as f32 / 120.;
                            if frame == 10 {
                                pet_key(core, 81, true, now); reference.key(81, true, now, &s);
                            }
                            if [30, 40, 70].contains(&frame) {
                                pet_click(core, now); reference.click(now, hand == "left");
                            }
                            if frame == 100 {
                                pet_key(core, 81, false, now); reference.key(81, false, now, &s);
                            }
                            if [32, 44, 75, 105].contains(&frame) {
                                // Mouse-up position save / window close / toggling topmost.
                                s.position = Some(config::Position { x: frame as f32, y: 20. });
                                s.always_on_top = !s.always_on_top;
                                let json = CString::new(serde_json::to_string(&s).unwrap()).unwrap();
                                assert!(pet_preview(core, json.as_ptr()));
                                assert!(pet_apply(core, json.as_ptr()));
                            }
                            reference.update(&s, &skin, Vec2::ZERO, 360., 420., false, now, 1./120.);
                            pet_render(core, 0., 0., 360., 420., false, now, 1./120.);
                            assert_eq!((*core).pose.arms, reference.arms, "{posture}/{hand}/{mode} frame={frame}");
                            assert_eq!((*core).pose.held_codes(), reference.held_codes());
                            assert_eq!((*core).pose.swing_progress, reference.swing_progress);
                            assert_eq!((*core).pose.squash, reference.squash);
                        }
                    }
                }
            }
            assert!((*core).settings.hyp_quick_chat);
            pet_destroy(core);
            let reopened = pet_create(path.as_ptr());
            assert!((*reopened).settings.hyp_quick_chat);
            pet_destroy(reopened);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn changing_only_main_hand_resets_swing_but_retains_held_keys() {
        let mut s = Settings::first_run_defaults();
        let mut core = PetCore { settings:s.clone(), store:Store::new(PathBuf::new()),
            skin:Skin::load(render::DEFAULT_SKIN).unwrap(), pose:Pose::new(&s),
            frame:Frame::new(1,1), message:CString::default(), json:CString::default() };
        core.pose.key(81, true, 0., &s); core.pose.click(0., false);
        s.main_hand = "left".into(); core.update_settings(s.clone());
        assert_eq!(core.pose.held_codes(), vec![81]);
        assert_eq!(core.pose.swing_progress, 0.);
        s.keyboard_enabled = false; core.update_settings(s);
        assert!(core.pose.held_codes().is_empty());
    }
    #[test]
    fn macos_bridge_renders_persists_and_rejects_invalid_skin() {
        let dir = std::env::temp_dir().join(format!("pet-macos-bridge-{}",std::process::id()));
        let path = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let core = pet_create(path.as_ptr());
            assert!(!core.is_null());
            let json = CStr::from_ptr(pet_settings(core)).to_owned();
            assert!(pet_apply(core, json.as_ptr()));
            assert!(!pet_render(core, 180., 160., 360., 420., false, 0., 0.016).is_null());
            assert!((*core).frame.pixels.chunks_exact(4).any(|p| p[3] > 0));
            assert!(!pet_hit(core, -1., 0.));
            assert!(!pet_import(core, path.as_ptr()));
            assert!(!CStr::from_ptr(pet_error(core)).is_empty());
            assert!(pet_reset(core));
            pet_destroy(core);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
