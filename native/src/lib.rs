//! Original renderer exposed through a main-thread-only C ABI for AppKit.
#![allow(dead_code)]
mod config;
mod keyboard;
mod render;
use config::{Settings, Store};
use glam::Vec2;
use render::{Cape, Frame, Gesture, Pose, Skin};
use std::{ffi::{c_char, CStr, CString}, path::PathBuf, ptr};

pub struct PetCore {
    settings: Settings,
    store: Store,
    skin: Skin,
    cape: Option<Cape>,
    pose: Pose,
    frame: Frame,
    message: CString,
    json: CString,
    hold: HoldTracker,
    pat_rng: u64,
}
/// Same rule as the Windows `HoldTracker`: a left press on the pet that stays
/// within `tolerance` for 0.5 s starts the long-press pat; moving or releasing ends it.
#[derive(Default)]
struct HoldTracker {
    press: Option<(f32, Vec2)>,
    active: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoldEvent { None, Start, End }
const HOLD_DELAY_SECONDS: f32 = 0.5;
impl HoldTracker {
    fn press(&mut self, now: f32, at: Vec2) { self.press = Some((now, at)); self.active = false; }
    fn cancel(&mut self) -> bool {
        let had = self.press.is_some() || self.active;
        self.press = None; self.active = false;
        had
    }
    fn update(&mut self, now: f32, down: bool, at: Vec2, tolerance: f32) -> HoldEvent {
        let Some((pressed_at, origin)) = self.press else {
            if self.active { self.active = false; return HoldEvent::End; }
            return HoldEvent::None;
        };
        let moved = (at.x - origin.x).abs() > tolerance || (at.y - origin.y).abs() > tolerance;
        if moved || !down {
            let was_active = self.active;
            self.press = None; self.active = false;
            return if was_active { HoldEvent::End } else { HoldEvent::None };
        }
        if !self.active && now - pressed_at >= HOLD_DELAY_SECONDS {
            self.active = true;
            return HoldEvent::Start;
        }
        HoldEvent::None
    }
}
impl PetCore {
    /// xorshift64, like the Windows PAT sound choice.
    fn pat_rand(&mut self) -> u32 {
        let mut x = self.pat_rng;
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        self.pat_rng = x;
        (x >> 32) as u32
    }
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
    let cape = std::fs::read(&settings.cape_path).ok().and_then(|bytes| Cape::load(&bytes).ok());
    let pose = Pose::new(&settings);
    Box::into_raw(Box::new(PetCore {settings, store, skin, cape, pose, frame: Frame::new(450, 525), message: CString::default(), json: CString::default(),
        hold: HoldTracker::default(),
        pat_rng: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64) | 1}))
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
/// Import a local skin. Clear any cape belonging to the previous account.
#[no_mangle]
pub unsafe extern "C" fn pet_import(core: *mut PetCore, path: *const c_char) -> bool {
    pet_import_profile(core, path, ptr::null(), ptr::null(), ptr::null())
}
/// A null cape path means that the new profile has no cape.
#[no_mangle]
pub unsafe extern "C" fn pet_import_profile(core: *mut PetCore, skin_path: *const c_char,
    cape_path: *const c_char, name: *const c_char, model: *const c_char) -> bool {
    let (Some(c), Some(path)) = (core.as_mut(), string(skin_path)) else { return false; };
    let result = (|| -> Result<(Skin, Option<Cape>, Settings), String> {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let skin = Skin::load(&bytes)?;
        let cape_data = string(cape_path).map(|p| std::fs::read(p).map_err(|e| e.to_string())).transpose()?;
        let cape = cape_data.as_deref().map(Cape::load).transpose()?;
        let mut next = c.settings.clone();
        next.skin_name = string(name).unwrap_or_else(|| PathBuf::from(&path).file_name().unwrap_or_default().to_string_lossy().into_owned());
        if let Some(model) = string(model) { next.arm_model = model; }
        if skin.legacy { next.arm_model = "classic".into(); }
        next.clean();
        // A unique generation keeps active files unchanged until settings commit.
        static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = format!("{}-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),
            GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let skin_dest = c.store.dir.join(format!("profile-skin-{id}.png"));
        let cape_dest = c.store.dir.join(format!("profile-cape-{id}.png"));
        config::atomic(&skin_dest, &bytes)?;
        if let Some(bytes) = cape_data {
            if let Err(e) = config::atomic(&cape_dest, &bytes) {
                let _ = std::fs::remove_file(&skin_dest); return Err(e);
            }
            next.cape_path = cape_dest.to_string_lossy().into_owned();
        } else { next.cape_path.clear(); }
        next.skin_path = skin_dest.to_string_lossy().into_owned();
        // On a partial filesystem failure retain these files: settings.json may
        // already have committed even if writing its backup failed.
        c.store.save(&next)?;
        Ok((skin, cape, next))
    })();
    match result {
        Ok((skin, cape, next)) => {
            let previous = [c.settings.skin_path.clone(), c.settings.cape_path.clone()];
            c.update_settings(next); c.skin = skin; c.cape = cape;
            c.message = CString::default();
            // Remove only files created by this importer, never user source files.
            for path in previous {
                let path = PathBuf::from(path);
                if path.parent() == Some(c.store.dir.as_path())
                    && path.file_name().and_then(|s| s.to_str()).is_some_and(|s|
                        s.starts_with("profile-skin-") || s.starts_with("profile-cape-")) {
                    let _ = std::fs::remove_file(path);
                }
            }
            true
        },
        Err(e) => c.error(e),
    }
}
#[no_mangle]
pub unsafe extern "C" fn pet_restore_skin(core: *mut PetCore) -> bool {
    let Some(c) = core.as_mut() else { return false; };
    let mut next = c.settings.clone();
    next.skin_path.clear();
    next.skin_name = "内置 · 林间旅人".into();
    // Match the Windows restore-skin command: keep poses, cape, startup and
    // interaction preferences. Commit successfully before changing the render.
    if let Err(e) = c.store.save(&next) { return c.error(e); }
    c.update_settings(next);
    c.skin = Skin::load(render::DEFAULT_SKIN).unwrap();
    c.message = CString::default();
    true
}
#[no_mangle]
pub unsafe extern "C" fn pet_reset(core: *mut PetCore) -> bool {
    let Some(c) = core.as_mut() else { return false; };
    c.settings = Settings::first_run_defaults();
    c.skin = Skin::load(render::DEFAULT_SKIN).unwrap();
    c.cape = None;
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
/// These entry points are main-thread only, like the rest of this ABI.
/// Mirrors the Windows `mouse_press`: a left press on the pet arms the long-press
/// pat; with PAT on, a press on the pet shows the hand and returns which meow to
/// play (0 or 1), otherwise -1. `x`/`y` are screen points, compared by `pet_hold_update`.
#[no_mangle]
pub unsafe extern "C" fn pet_mouse(core: *mut PetCore, button: u32, over: bool, x: f32, y: f32, now: f32) -> i32 {
    let Some(c) = core.as_mut() else { return -1; };
    if button == 0 && over { c.hold.press(now, Vec2::new(x, y)); }
    if c.settings.tease_enabled && over { c.pose.tease(now); }
    else { c.pose.click(now, c.settings.swing_left(button as usize)); }
    if c.settings.pat_enabled && over {
        c.pose.pat(now);
        return (c.pat_rand() & 1) as i32;
    }
    -1
}
/// Polled every frame with the left button state. Returns 1 while a press may
/// still become a long-press pat or is one (the window must not move yet), else 0.
#[no_mangle]
pub unsafe extern "C" fn pet_hold_update(core: *mut PetCore, down: bool, x: f32, y: f32, tolerance: f32, now: f32) -> u32 {
    let Some(c) = core.as_mut() else { return 0; };
    match c.hold.update(now, down, Vec2::new(x, y), tolerance) {
        HoldEvent::Start => c.pose.start_hold(now),
        HoldEvent::End => c.pose.end_hold(now),
        HoldEvent::None => {}
    }
    (c.hold.press.is_some() || c.hold.active) as u32
}
/// Hidden pet, open menu or opening the console: drop a pending or running hold.
#[no_mangle]
pub unsafe extern "C" fn pet_hold_cancel(core: *mut PetCore, now: f32) {
    let Some(c) = core.as_mut() else { return; };
    c.hold.cancel();
    if c.pose.hold_active() { c.pose.end_hold(now); }
}
#[no_mangle]
pub unsafe extern "C" fn pet_feed(core: *mut PetCore, now: f32) {
    if let Some(c) = core.as_mut() { c.pose.feed(now); }
}
#[no_mangle]
pub unsafe extern "C" fn pet_music(core: *mut PetCore, playing: bool) {
    if let Some(c) = core.as_mut() { c.pose.music_playing = playing; }
}
#[no_mangle]
pub unsafe extern "C" fn pet_gesture(core: *mut PetCore, kind: u32, now: f32) -> bool {
    let Some(c) = core.as_mut() else { return false; };
    let Some(&kind) = [Gesture::Wave, Gesture::Bounce, Gesture::Shy, Gesture::Clap].get(kind as usize) else { return false; };
    c.pose.trigger_gesture(kind, now);
    true
}
/// `now` lets the occasional-action deadline wake the idle frame timer.
#[no_mangle]
pub unsafe extern "C" fn pet_needs_frames(core: *mut PetCore, now: f32) -> bool {
    core.as_ref().is_some_and(|c| c.pose.needs_frames()
        || c.pose.gesture_due(&c.settings, now)
        || c.settings.breathing_animation
        || (c.settings.da_blink && c.skin.eyes.is_some())
        || (c.settings.da_dance && c.settings.posture == "standing")
        || (c.settings.music_notes && c.pose.music_playing))
}
#[no_mangle]
pub unsafe extern "C" fn pet_pixels(core: *mut PetCore) -> *const u8 {
    core.as_ref().map_or(ptr::null(), |c| c.frame.pixels.as_ptr())
}
#[no_mangle]
pub unsafe extern "C" fn pet_set_glyphs(bytes: *const u8, length: usize) -> bool {
    if bytes.is_null() || length != 95 * 576 { return false; }
    render::install_glyphs(std::slice::from_raw_parts(bytes, length))
}
#[no_mangle]
pub unsafe extern "C" fn pet_render(core: *mut PetCore, x: f32, y: f32, width: f32, height: f32, shift: bool, now: f32, dt: f32) -> *const u8 {
    let Some(c) = core.as_mut() else { return ptr::null(); };
    c.pose.update(&c.settings, &c.skin, Vec2::new(x, y), width, height, shift, now, dt);
    c.frame.draw_ext(&c.settings, &c.skin, c.cape.as_ref(), &c.pose);
    c.frame.pixels.as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn pet_hit(core: *mut PetCore, x: f32, y: f32) -> bool {
    let Some(c) = core.as_ref() else { return false; };
    if x < 0. || y < 0. || x >= 1. || y >= 1. { return false; }
    c.frame.pixels[((y * 525.) as usize * 450 + (x * 450.) as usize) * 4 + 3] != 0
}

#[cfg(test)]
mod ffi_tests {
    use super::*;
    #[test]
    fn restoring_builtin_skin_keeps_other_preferences() {
        let dir = std::env::temp_dir().join(format!("pet-restore-skin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let core = pet_create(path.as_ptr());
            assert!(!core.is_null());
            (*core).settings.skin_name = "Imported skin".into();
            (*core).settings.skin_path = "custom.png".into();
            (*core).settings.cape_path = "keep-cape.png".into();
            (*core).settings.posture = "custom".into();
            (*core).settings.autostart = true;
            (*core).settings.music_notes = true;
            let mut expected = (*core).settings.clone();
            expected.skin_name = "内置 · 林间旅人".into();
            expected.skin_path.clear();
            assert!(pet_restore_skin(core));
            assert_eq!(serde_json::to_value(&(*core).settings).unwrap(), serde_json::to_value(&expected).unwrap());
            pet_destroy(core);
            let reopened = pet_create(path.as_ptr());
            assert_eq!(serde_json::to_value(&(*reopened).settings).unwrap(), serde_json::to_value(&expected).unwrap());
            pet_destroy(reopened);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn macos_pat_click_and_long_press_hold() {
        let dir = std::env::temp_dir().join(format!("pet-pat-ffi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let core = pet_create(path.as_ptr());
            (*core).settings.pat_enabled = false;
            assert_eq!(pet_mouse(core, 0, true, 10., 10., 1.), -1, "PAT off plays no sound");
            pet_hold_cancel(core, 1.);
            (*core).settings.pat_enabled = true;
            assert_eq!(pet_mouse(core, 0, false, 10., 10., 2.), -1, "missing the pet plays no sound");
            let mut sounds = std::collections::HashSet::new();
            for i in 0..40 { sounds.insert(pet_mouse(core, 1, true, 0., 0., 3. + i as f32)); }
            assert_eq!(sounds, [0, 1].into_iter().collect(), "both meows are chosen at random");
            assert!(pet_needs_frames(core, 43.), "PAT hand animates");
            // Hold: 0.5 s without moving more than the tolerance.
            pet_hold_cancel(core, 50.);
            pet_mouse(core, 0, true, 100., 100., 50.);
            assert_eq!(pet_hold_update(core, true, 101., 100., 30., 50.49), 1);
            assert!(!(*core).pose.hold_active());
            assert_eq!(pet_hold_update(core, true, 120., 110., 30., 50.5), 1);
            assert!((*core).pose.hold_active(), "hold starts after 0.5 s");
            assert_eq!(pet_hold_update(core, false, 120., 110., 30., 51.), 0);
            // Moving past the tolerance before 0.5 s never starts a hold.
            pet_render(core, 0., 0., 360., 420., false, 53., 0.016);
            pet_mouse(core, 0, true, 100., 100., 60.);
            assert_eq!(pet_hold_update(core, true, 140., 100., 30., 60.2), 0, "drag releases the window");
            assert_eq!(pet_hold_update(core, true, 140., 100., 30., 60.8), 0);
            // Right button or a miss never arms a hold; cancel ends a running one.
            pet_mouse(core, 1, true, 100., 100., 70.);
            assert_eq!(pet_hold_update(core, true, 100., 100., 30., 71.), 0);
            pet_mouse(core, 0, true, 100., 100., 72.);
            pet_hold_update(core, true, 100., 100., 30., 72.6);
            assert!((*core).pose.holding);
            pet_hold_cancel(core, 72.7);
            assert_eq!(pet_hold_update(core, true, 100., 100., 30., 72.8), 0);
            pet_destroy(core);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn macos_v25_events_frames_and_settings() {
        let dir = std::env::temp_dir().join(format!("pet-v25-ffi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        unsafe {
            let core = pet_create(path.as_ptr());
            assert!(!core.is_null());
            (*core).settings.track_mouse = false;
            (*core).settings.da_blink = false;
            (*core).settings.da_dance = false;
            (*core).settings.breathing_animation = false;
            assert!(!pet_needs_frames(core,0.));
            for mode in ["left","right","mapped","reverse"] {
                (*core).settings.main_hand = mode.into();
                for button in 0..5 {
                    (*core).pose = Pose::new(&(*core).settings);
                    let mut expected = Pose::new(&(*core).settings);
                    expected.click(0., (*core).settings.swing_left(button));
                    pet_mouse(core, button as u32, false, 0., 0., 0.);
                    for frame in 0..8 {
                        let now = frame as f32 / 120.;
                        expected.update(&(*core).settings, &(*core).skin, Vec2::ZERO,360.,420.,false,now,1./120.);
                        pet_render(core,0.,0.,360.,420.,false,now,1./120.);
                        assert_eq!((*core).pose.arms,expected.arms, "{mode}/{button}");
                    }
                }
            }
            pet_feed(core,1.);
            assert!(pet_needs_frames(core,0.));
            assert_eq!((*core).pose.effect_counts()[2],1);
            (*core).settings.tease_enabled=true;
            assert_eq!(pet_mouse(core,2,true,0.,0.,1.1),-1);
            assert!((*core).pose.interaction_state().0);
            assert_eq!((*core).pose.effect_counts()[2],0);
            (*core).settings.music_notes=true;
            pet_music(core,true);
            pet_render(core,0.,0.,360.,420.,false,2.,0.016);
            assert_eq!((*core).pose.effect_counts()[1],1);
            assert!(pet_needs_frames(core,0.));
            pet_music(core,false);
            pet_render(core,0.,0.,360.,420.,false,3.,0.016);
            assert_eq!((*core).pose.effect_counts()[1],0);
            (*core).settings.breathing_animation=true;
            assert!(pet_needs_frames(core,0.));
            (*core).settings.breathing_animation=false;
            pet_render(core,0.,0.,360.,420.,false,10.,0.016);
            assert!(!pet_needs_frames(core,10.));
            (*core).settings.occasional_tilt=true;
            assert!(pet_needs_frames(core,10.), "mask change wakes the frame timer");
            pet_render(core,0.,0.,360.,420.,false,10.,0.016);
            let due=(*core).pose.next_gesture_at().unwrap();
            assert!(!pet_needs_frames(core,due-0.1));
            assert!(pet_needs_frames(core,due));
            (*core).settings.occasional_tilt=false;
            assert!(!pet_gesture(core,4,20.));
            assert!(pet_gesture(core,0,20.));
            assert!(pet_needs_frames(core,20.));
            (*core).settings.breathing_animation=true;
            assert_eq!(pet_pixels(core),(*core).frame.pixels.as_ptr());
            let json=pet_settings(core);
            assert!(!CStr::from_ptr(json).to_str().unwrap().contains("startupPriority"));
            pet_destroy(core);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn macos_profile_import_cape_toggle_failure_restart_and_clear() {
        let dir = std::env::temp_dir().join(format!("pet-macos-cape-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let skin_path = dir.join("source-skin.png");
        std::fs::write(&skin_path,render::DEFAULT_SKIN).unwrap();
        let cape_path = dir.join("source-cape.png");
        let mut tex = image::RgbaImage::new(64,32);
        for y in 1..17 { for x in 1..11 { tex.put_pixel(x,y,image::Rgba([220,30,30,255])); } }
        tex.save(&cape_path).unwrap();
        let root = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
        let skin = CString::new(skin_path.to_string_lossy().as_bytes()).unwrap();
        let cape = CString::new(cape_path.to_string_lossy().as_bytes()).unwrap();
        let name = CString::new("TestProfile").unwrap(); let model = CString::new("slim").unwrap();
        unsafe {
            let core = pet_create(root.as_ptr()); assert!(!core.is_null());
            assert!(pet_import_profile(core,skin.as_ptr(),cape.as_ptr(),name.as_ptr(),model.as_ptr()));
            assert!((*core).cape.is_some()); assert_eq!((*core).settings.skin_name,"TestProfile");
            let old_settings = std::fs::read(dir.join("settings.json")).unwrap();
            // A bad cape must not import the new skin/name or discard the old cape.
            assert!(!pet_import_profile(core,skin.as_ptr(),skin.as_ptr(),name.as_ptr(),model.as_ptr()));
            assert_eq!(std::fs::read(dir.join("settings.json")).unwrap(),old_settings);
            assert!((*core).cape.is_some());
            (*core).settings.model_rotation = [0.,180.,0.];
            (*core).settings.track_mouse = false;
            pet_render(core,0.,0.,360.,420.,false,0.,0.016);
            let shown = (*core).frame.pixels.clone();
            (*core).settings.show_cape = false;
            pet_render(core,0.,0.,360.,420.,false,0.,0.016);
            assert_ne!((*core).frame.pixels,shown);
            pet_destroy(core);
            let core = pet_create(root.as_ptr()); assert!((*core).cape.is_some());
            let old_cape = (*core).settings.cape_path.clone();
            assert!(pet_import_profile(core,skin.as_ptr(),ptr::null(),name.as_ptr(),model.as_ptr()));
            assert!((*core).cape.is_none()); assert!((&(*core).settings.cape_path).is_empty());
            assert!(!std::path::Path::new(&old_cape).exists());
            assert!(cape_path.exists()); // Never delete user source textures.
            pet_destroy(core);
            let core = pet_create(root.as_ptr()); assert!((*core).cape.is_none());
            assert!(pet_import_profile(core,skin.as_ptr(),cape.as_ptr(),name.as_ptr(),model.as_ptr()));
            assert!(pet_import(core,skin.as_ptr())); assert!((*core).cape.is_none());
            assert!(pet_import_profile(core,skin.as_ptr(),cape.as_ptr(),name.as_ptr(),model.as_ptr()));
            assert!(pet_reset(core)); assert!((*core).cape.is_none());
            assert!((&(*core).settings.cape_path).is_empty());
            pet_destroy(core);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
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
            skin:Skin::load(render::DEFAULT_SKIN).unwrap(), cape:None, pose:Pose::new(&s),
            frame:Frame::new(1,1), message:CString::default(), json:CString::default(),
            hold:HoldTracker::default(), pat_rng:1 };
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
        std::fs::create_dir_all(&dir).unwrap();
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
