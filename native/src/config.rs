use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
pub const FPS_OPTIONS: [u32; 7] = [30, 60, 90, 120, 144, 165, 240];
/// 进程优先级档位，与 Windows 的 SetPriorityClass 一一对应。
pub const PRIORITY_OPTIONS: [&str; 5] = ["idle", "below", "normal", "above", "high"];
/// 开机自启的启动延迟档位（秒）；0 表示立即出现。
pub const AUTOSTART_DELAYS: [u32; 6] = [0, 5, 15, 30, 60, 120];
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomPose {
    pub head: [f32; 3],
    pub body: [f32; 3],
    pub left_arm: [f32; 3],
    pub right_arm: [f32; 3],
    pub left_leg: [f32; 3],
    pub right_leg: [f32; 3],
}
impl CustomPose {
    pub fn row(&self, row: usize) -> &[f32] {
        match row {
            0 => &self.head,
            1 => &self.body,
            2 => &self.left_arm,
            3 => &self.right_arm,
            4 => &self.left_leg,
            _ => &self.right_leg,
        }
    }
    pub fn row_mut(&mut self, row: usize) -> &mut [f32] {
        match row {
            0 => &mut self.head,
            1 => &mut self.body,
            2 => &mut self.left_arm,
            3 => &mut self.right_arm,
            4 => &mut self.left_leg,
            _ => &mut self.right_leg,
        }
    }
    fn clean(&mut self) {
        for row in 0..6 {
            for value in self.row_mut(row).iter_mut() {
                *value = if value.is_finite() {
                    value.clamp(-180., 180.)
                } else {
                    0.
                };
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub posture: String,
    pub arm_model: String,
    pub typing_mode: String,
    pub typing_squash: f32,
    pub scale: f32,
    pub always_on_top: bool,
    pub click_through: bool,
    pub track_mouse: bool,
    pub keyboard_enabled: bool,
    pub hyp_quick_chat: bool,
    pub autostart: bool,
    pub show_cape: bool,
    pub cape_path: String,
    pub autostart_delay: u32,
    pub startup_priority: String,
    pub outer_layer: bool,
    pub fps: u32,
    pub skin_name: String,
    pub skin_path: String,
    pub position: Option<Position>,
    pub head_sensitivity_x: f32,
    pub head_sensitivity_y: f32,
    pub head_range_x: f32,
    pub head_range_y: f32,
    pub head_center_x: f32,
    pub head_center_y: f32,
    pub model_rotation: [f32; 3],
    pub main_hand: String,
    pub custom_pose: CustomPose,
    pub custom_base: String,
    pub custom_sitting_pose: CustomPose,
    pub model_mode: String,
    pub blockbench_path: String,
    pub blockbench_name: String,
    pub blockbench_animation: String,
    pub blockbench_loop: bool,
    pub blockbench_speed: f32,
    pub blockbench_head_track: bool,
    pub keyboard_bounce: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            posture: "standing".into(),
            arm_model: "classic".into(),
            typing_mode: "precise".into(),
            typing_squash: 5.,
            scale: 1.,
            always_on_top: true,
            click_through: false,
            track_mouse: true,
            keyboard_enabled: true,
            hyp_quick_chat: false,
            autostart: false,
            show_cape: true,
            cape_path: String::new(),
            autostart_delay: 0,
            startup_priority: "normal".into(),
            outer_layer: true,
            fps: 60,
            skin_name: "内置 · 林间旅人".into(),
            skin_path: String::new(),
            position: None,
            head_sensitivity_x: 100.,
            head_sensitivity_y: 100.,
            head_range_x: 19.5,
            head_range_y: 53.,
            head_center_x: 0.,
            head_center_y: 0.,
            model_rotation: [0.; 3],
            main_hand: "right".into(),
            custom_pose: CustomPose::default(),
            custom_base: "standing".into(),
            custom_sitting_pose: CustomPose::default(),
            model_mode: "player".into(),
            blockbench_path: String::new(),
            blockbench_name: String::new(),
            blockbench_animation: String::new(),
            blockbench_loop: true,
            blockbench_speed: 1.0,
            blockbench_head_track: true,
            keyboard_bounce: true,
        }
    }
}
impl Settings {
    /// First-run defaults are separate from legacy field defaults so upgrading
    /// an older, partial settings file does not silently change its preferences.
    pub fn first_run_defaults() -> Self {
        let mut settings: Self = serde_json::from_str(include_str!("../default-settings.json"))
            .expect("bundled defaults must be valid settings");
        settings.clean();
        settings
    }
    pub fn is_sitting(&self) -> bool {
        self.posture == "sitting" || (self.posture == "custom" && self.custom_base == "sitting")
    }
    pub fn active_custom_pose(&self) -> &CustomPose {
        if self.custom_base == "sitting" {
            &self.custom_sitting_pose
        } else {
            &self.custom_pose
        }
    }
    pub fn active_custom_pose_mut(&mut self) -> &mut CustomPose {
        if self.custom_base == "sitting" {
            &mut self.custom_sitting_pose
        } else {
            &mut self.custom_pose
        }
    }
    pub fn clean(&mut self) {
        if self.posture != "sitting" && self.posture != "custom" {
            self.posture = "standing".into();
        }
        if self.arm_model != "slim" {
            self.arm_model = "classic".into();
        }
        if self.typing_mode != "alternating" {
            self.typing_mode = "precise".into();
        }
        self.scale = self.scale.clamp(0.65, 1.8);
        self.typing_squash = if self.typing_squash.is_finite() {
            self.typing_squash.clamp(0., 30.)
        } else {
            5.
        };
        self.fps = *FPS_OPTIONS
            .iter()
            .min_by_key(|&&fps| fps.abs_diff(self.fps))
            .unwrap();
        self.head_sensitivity_x = self.head_sensitivity_x.clamp(0., 300.);
        self.head_sensitivity_y = self.head_sensitivity_y.clamp(0., 300.);
        self.head_range_x = self.head_range_x.clamp(0., 85.);
        self.head_range_y = self.head_range_y.clamp(0., 85.);
        self.head_center_x = self.head_center_x.clamp(-60., 60.);
        self.head_center_y = self.head_center_y.clamp(-60., 60.);
        if self.main_hand != "left" {
            self.main_hand = "right".into();
        }
        if !PRIORITY_OPTIONS.contains(&self.startup_priority.as_str()) {
            self.startup_priority = "normal".into();
        }
        self.autostart_delay = *AUTOSTART_DELAYS
            .iter()
            .min_by_key(|&&delay| delay.abs_diff(self.autostart_delay))
            .unwrap();
        for v in &mut self.model_rotation {
            *v = if v.is_finite() {
                v.clamp(-180., 180.)
            } else {
                0.
            };
        }
        self.custom_pose.clean();
        if self.custom_base != "sitting" {
            self.custom_base = "standing".into();
        }
        self.custom_sitting_pose.clean();
        if self.model_mode != "blockbench" {
            self.model_mode = "player".into();
        }
        self.blockbench_speed = if self.blockbench_speed.is_finite() {
            self.blockbench_speed.clamp(0.1, 5.0)
        } else {
            1.0
        };
    }
    pub fn head(&self) -> serde_json::Value {
        serde_json::json!({"headSensitivityX":self.head_sensitivity_x,"headSensitivityY":self.head_sensitivity_y,"headRangeX":self.head_range_x,"headRangeY":self.head_range_y,"headCenterX":self.head_center_x,"headCenterY":self.head_center_y})
    }
    pub fn set_head(&mut self, v: &serde_json::Value) -> bool {
        let names = [
            "headSensitivityX",
            "headSensitivityY",
            "headRangeX",
            "headRangeY",
            "headCenterX",
            "headCenterY",
        ];
        let values: Option<Vec<f32>> = names
            .iter()
            .map(|k| {
                v.get(k)?
                    .as_f64()
                    .filter(|x| x.is_finite())
                    .map(|x| x as f32)
            })
            .collect();
        if let Some(a) = values {
            self.head_sensitivity_x = a[0];
            self.head_sensitivity_y = a[1];
            self.head_range_x = a[2];
            self.head_range_y = a[3];
            self.head_center_x = a[4];
            self.head_center_y = a[5];
            self.clean();
            true
        } else {
            false
        }
    }
}
pub fn atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    file.write_all(data).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    #[cfg(windows)]
    {
        let a: Vec<u16> = tmp.as_os_str().encode_wide_null();
        let b: Vec<u16> = path.as_os_str().encode_wide_null();
        if unsafe {
            MoveFileExW(
                a.as_ptr(),
                b.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(not(windows))]
    {
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        fs::File::open(path.parent().unwrap())
            .and_then(|dir| dir.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(windows)]
trait Wide {
    fn encode_wide_null(&self) -> Vec<u16>;
}
#[cfg(windows)]
impl Wide for std::ffi::OsStr {
    fn encode_wide_null(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}
pub struct Store {
    pub dir: PathBuf,
    pub notice: String,
    pub read_error: Option<String>,
    last: String,
}
impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            notice: String::new(),
            read_error: None,
            last: String::new(),
        }
    }
    pub fn load(&mut self) -> Settings {
        let mut value = None;
        let mut damaged = false;
        for (i, name) in [
            "settings.json",
            "settings.json.tmp",
            "settings.json.bak",
            "settings.json.bak.tmp",
        ]
        .iter()
        .enumerate()
        {
            let p = self.dir.join(name);
            match fs::read(&p) {
                Ok(bytes) => {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        let mut clean = serde_json::to_value(Settings::default()).unwrap();
                        if v.is_object()
                            && clean
                                .as_object()
                                .unwrap()
                                .keys()
                                .any(|k| v.get(k).is_some())
                        {
                            // An invalid individual field must not discard unrelated saved preferences.
                            for (k, default) in clean.as_object_mut().unwrap() {
                                if let Some(candidate) = v.get(k) {
                                    let valid = if k == "position" {
                                        candidate.is_null()
                                            || serde_json::from_value::<Position>(candidate.clone())
                                                .is_ok()
                                    } else if k == "modelRotation" {
                                        serde_json::from_value::<[f32; 3]>(candidate.clone())
                                            .is_ok()
                                    } else if k == "customPose" || k == "customSittingPose" {
                                        serde_json::from_value::<CustomPose>(candidate.clone())
                                            .is_ok()
                                    } else {
                                        (default.is_boolean() && candidate.is_boolean())
                                            || (default.is_string() && candidate.is_string())
                                            || (default.is_number() && candidate.is_number())
                                    };
                                    if valid {
                                        *default = candidate.clone();
                                    }
                                }
                            }
                            // fps is stored as an integer even in older configurations.
                            if clean["fps"].as_u64().is_none() {
                                clean["fps"] = serde_json::json!(60);
                            }
                            value = serde_json::from_value::<Settings>(clean).ok();
                            if value.is_some() {
                                if i > 0 {
                                    self.notice = "已从备份恢复设置。".into();
                                }
                                break;
                            }
                        }
                    }
                    damaged = true;
                    let _ = fs::copy(
                        &p,
                        p.with_extension(format!("corrupt-{}", std::process::id())),
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    self.read_error = Some(format!(
                        "无法读取设置 {}：{}。为保护原有设置，本次不启动。",
                        p.display(),
                        e
                    ));
                    return Settings::default();
                }
            }
        }
        if value.is_none() && damaged {
            self.notice = "设置及备份已损坏，已保留原文件供排查；本次使用默认设置。".into();
        }
        let mut s = value.unwrap_or_else(Settings::first_run_defaults);
        for name in ["head-follow.json", "head-follow.json.bak"] {
            match fs::read(self.dir.join(name)) {
                Ok(b) => {
                    if let Ok(v) = serde_json::from_slice(&b) {
                        let old = s.head();
                        if s.set_head(&v) {
                            if old != s.head() {
                                self.notice = "已从独立保存的头部参数恢复设置。".into();
                            }
                            break;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    self.read_error = Some(format!(
                        "无法读取头部参数：{}。为保护原有设置，本次不启动。",
                        e
                    ));
                    break;
                }
            }
        }
        s.clean();
        s
    }
    pub fn save(&mut self, s: &Settings) -> Result<(), String> {
        if let Some(e) = &self.read_error {
            return Err(e.clone());
        }
        let data = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
        if data == self.last {
            return Ok(());
        }
        let head = serde_json::to_vec_pretty(&s.head()).map_err(|e| e.to_string())?;
        for name in ["head-follow.json", "head-follow.json.bak"] {
            let p = self.dir.join(name);
            if fs::read(&p).ok().as_deref() != Some(head.as_slice()) {
                atomic(&p, &head)?;
            }
        }
        for name in ["settings.json", "settings.json.bak"] {
            atomic(&self.dir.join(name), data.as_bytes())?;
        }
        self.last = data;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_run_uses_release_preferences_without_personal_skin() {
        let dir = std::env::temp_dir().join(format!("pet-release-defaults-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let expected = Settings::first_run_defaults();
        let mut store = Store::new(dir.clone());
        assert_eq!(store.load(), expected);
        assert_eq!(expected.posture, "sitting");
        assert_eq!(expected.fps, 120);
        assert_eq!(expected.main_hand, "right");
        assert_eq!(expected.typing_squash, 20.);
        assert!(expected.click_through);
        assert!(!expected.hyp_quick_chat);
        assert!(expected.skin_path.is_empty());
        assert_eq!(expected.skin_name, Settings::default().skin_name);
        assert!(expected.position.is_none());
        store.save(&expected).unwrap();
        assert_eq!(Store::new(dir.clone()).load(), expected);
        // Existing partial configurations keep their old missing-field defaults.
        fs::write(
            dir.join("settings.json"),
            br#"{"fps":60,"clickThrough":false}"#,
        )
        .unwrap();
        let loaded = Store::new(dir.clone()).load();
        assert_eq!(loaded.fps, 60);
        assert!(!loaded.click_through);
        assert!(!loaded.hyp_quick_chat);
        assert_eq!(loaded.typing_squash, 5.);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn separate_custom_standing_and_sitting_persist() {
        let dir = std::env::temp_dir().join(format!("pet-two-poses-{}", std::process::id()));
        let mut s = Settings::default();
        s.posture = "custom".into();
        s.custom_pose.left_arm = [-70., 30., 20.];
        s.custom_base = "sitting".into();
        s.active_custom_pose_mut().left_arm = [-15., 10., 5.];
        assert!(s.is_sitting());
        Store::new(dir.clone()).save(&s).unwrap();
        let mut loaded = Store::new(dir.clone()).load();
        assert_eq!(loaded, s);
        loaded.custom_base = "standing".into();
        assert_eq!(loaded.active_custom_pose().left_arm, [-70., 30., 20.]);
        assert!(!loaded.is_sitting());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn previous_custom_pose_migrates_as_standing() {
        let dir = std::env::temp_dir().join(format!("pet-pose-migration-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("settings.json"), br#"{"posture":"custom","customPose":{"leftArm":[-60,20,30]},"mainHand":"left","modelRotation":[10,20,30]}"#).unwrap();
        let s = Store::new(dir.clone()).load();
        assert_eq!(s.custom_base, "standing");
        assert_eq!(s.custom_pose.left_arm, [-60., 20., 30.]);
        assert_eq!(s.custom_sitting_pose, CustomPose::default());
        assert_eq!(s.main_hand, "left");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn custom_pose_main_hand_and_angles_survive_restart_and_backup() {
        let dir = std::env::temp_dir().join(format!("pet-pose-test-{}", std::process::id()));
        let mut s = Settings::default();
        s.posture = "custom".into();
        s.main_hand = "left".into();
        s.model_rotation = [15., -75., 25.];
        for row in 0..6 {
            for axis in 0..3 {
                s.custom_pose.row_mut(row)[axis] = (row * 10 + axis) as f32 - 35.;
            }
        }
        let mut store = Store::new(dir.clone());
        store.save(&s).unwrap();
        assert_eq!(Store::new(dir.clone()).load(), s);
        fs::write(dir.join("settings.json"), b"{broken").unwrap();
        assert_eq!(Store::new(dir.clone()).load(), s);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn fps_range_and_persistence() {
        let dir = std::env::temp_dir().join(format!("pet-fps-test-{}", std::process::id()));
        let mut store = Store::new(dir.clone());
        for fps in FPS_OPTIONS {
            let mut s = Settings::default();
            s.fps = fps;
            s.clean();
            store.save(&s).unwrap();
            assert_eq!(Store::new(dir.clone()).load().fps, fps);
        }
        let mut s = Settings::default();
        s.fps = 0;
        s.clean();
        assert_eq!(s.fps, 30);
        s.fps = 999;
        s.clean();
        assert_eq!(s.fps, 240);
        s.fps = 173;
        s.clean();
        assert_eq!(s.fps, 165);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn typing_squash_migrates_and_survives_restart_and_backup() {
        let dir = std::env::temp_dir().join(format!("pet-squash-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            br#"{"posture":"sitting","scale":0.8}"#,
        )
        .unwrap();
        let mut store = Store::new(dir.clone());
        let mut s = store.load();
        assert_eq!(s.typing_squash, 5.);
        assert_eq!(s.posture, "sitting");
        assert_eq!(s.scale, 0.8);
        for value in [0., 5., 17., 30.] {
            s.typing_squash = value;
            store.save(&s).unwrap();
            assert_eq!(Store::new(dir.clone()).load(), s);
        }
        fs::write(dir.join("settings.json"), b"{broken").unwrap();
        assert_eq!(Store::new(dir.clone()).load(), s);
        for (input, expected) in [(-10., 0.), (100., 30.), (f32::NAN, 5.)] {
            s.typing_squash = input;
            s.clean();
            assert_eq!(s.typing_squash, expected);
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn malformed_field_keeps_other_preferences() {
        let dir = std::env::temp_dir().join(format!("pet-field-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            br#"{"posture":"sitting","scale":"invalid","headCenterX":-8,"headSensitivityY":40}"#,
        )
        .unwrap();
        let s = Store::new(dir.clone()).load();
        assert_eq!(s.posture, "sitting");
        assert_eq!(s.head_center_x, -8.);
        assert_eq!(s.head_sensitivity_y, 40.);
        assert_eq!(s.scale, 1.);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn read_failure_prevents_default_overwrite() {
        let dir = std::env::temp_dir().join(format!("pet-read-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("settings.json")).unwrap();
        let mut store = Store::new(dir.clone());
        let s = store.load();
        assert!(store.read_error.is_some());
        assert!(store.save(&s).is_err());
        assert!(!dir.join("head-follow.json").exists());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn autostart_preferences_default_off_and_clamp() {
        let mut s = Settings::default();
        assert!(!s.autostart);
        assert_eq!(s.autostart_delay, 0);
        assert_eq!(s.startup_priority, "normal");
        s.autostart_delay = 40;
        s.clean();
        assert_eq!(s.autostart_delay, 30);
        s.startup_priority = "bogus".into();
        s.clean();
        assert_eq!(s.startup_priority, "normal");
        let dir = std::env::temp_dir().join(format!("pet-autostart-test-{}", std::process::id()));
        s.autostart = true;
        s.autostart_delay = 60;
        s.startup_priority = "below".into();
        Store::new(dir.clone()).save(&s).unwrap();
        assert_eq!(Store::new(dir.clone()).load(), s);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn migrate() {
        let mut s: Settings =
            serde_json::from_str(r#"{"posture":"sitting","scale":0.8,"headCenterX":-8}"#).unwrap();
        assert_eq!(s.head_sensitivity_x, 100.);
        assert_eq!(s.head_center_x, -8.);
        s.set_head(&serde_json::json!({"headSensitivityX":40,"headSensitivityY":40,"headRangeX":85,"headRangeY":85,"headCenterX":-8,"headCenterY":-11}));
        assert_eq!(s.head_sensitivity_y, 40.);
        assert_eq!(s.scale, 0.8);
    }
    #[test]
    fn hyp_quick_chat_defaults_off_and_survives_restart_and_backup() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.hyp_quick_chat);
        let dir = std::env::temp_dir().join(format!("pet-hyp-test-{}", std::process::id()));
        let mut s = old;
        s.hyp_quick_chat = true;
        s.keyboard_enabled = false;
        Store::new(dir.clone()).save(&s).unwrap();
        assert_eq!(Store::new(dir.clone()).load(), s);
        fs::write(dir.join("settings.json"), b"{").unwrap();
        assert_eq!(Store::new(dir.clone()).load(), s);
        s.hyp_quick_chat = false;
        Store::new(dir.clone()).save(&s).unwrap();
        assert!(!Store::new(dir.clone()).load().hyp_quick_chat);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn recovery() {
        let dir = std::env::temp_dir().join(format!("pet-native-test-{}", std::process::id()));
        let mut st = Store::new(dir.clone());
        let mut s = Settings::default();
        s.head_sensitivity_x = 40.;
        s.head_center_y = -11.;
        st.save(&s).unwrap();
        fs::write(dir.join("settings.json"), b"{").unwrap();
        assert_eq!(Store::new(dir.clone()).load(), s);
        fs::write(
            dir.join("settings.json"),
            serde_json::to_vec(&Settings::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(Store::new(dir.clone()).load().head_center_y, -11.);
        let _ = fs::remove_dir_all(dir);
    }
}
