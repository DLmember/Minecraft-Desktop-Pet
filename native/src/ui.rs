use super::*;
use windows_sys::Win32::UI::Controls::Dialogs::*;
const SLIDER_IDS: [i32; 10] = [200, 201, 202, 203, 204, 205, 206, 207, 211, 507];
const SS_PATHELLIPSIS: u32 = 0x8000;
const SS_RIGHT: u32 = 2;
const TBM_GETPOS: u32 = WM_USER;
impl App {
    unsafe fn control(
        &mut self,
        class: &str,
        text: &str,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: i32,
    ) -> HWND {
        let d = self.panel_scale;
        let hwnd = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            (x as f32 * d) as i32,
            (y as f32 * d) as i32,
            (w as f32 * d) as i32,
            (h as f32 * d) as i32,
            self.panel,
            id as usize as HMENU,
            GetModuleHandleW(null()),
            null(),
        );
        self.layout.push((hwnd, [x, y, w, h]));
        self.page_controls.push((hwnd, self.building_page));
        SendMessageW(hwnd, WM_SETFONT, self.font as usize, 1);
        if id != 0 {
            self.controls.push((id, hwnd));
        }
        hwnd
    }
    pub(super) unsafe fn show_panel(&mut self) {
        if !self.panel.is_null() {
            ShowWindow(self.panel, SW_RESTORE);
            SetForegroundWindow(self.panel);
            return;
        }
        let class = wide("MinecraftDesktopPetSettings");
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(super::wndproc);
        wc.hInstance = GetModuleHandleW(null());
        wc.hIcon = self.icon;
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.hbrBackground = GetStockObject(WHITE_BRUSH) as HBRUSH;
        wc.lpszClassName = class.as_ptr();
        RegisterClassExW(&wc);
        let d = self.panel_fit(
            MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST),
            self.dpi,
        );
        self.panel_scale = d;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (760. * d) as i32,
            bottom: (786. * d) as i32,
        };
        AdjustWindowRectExForDpi(
            &mut rect,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            0,
            0,
            (d * 96.) as u32,
        );
        self.panel = CreateWindowExW(
            WS_EX_ACCEPTFILES,
            class.as_ptr(),
            wide(&format!("{} · 控制台", product_title())).as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if !self.font.is_null() {
            DeleteObject(self.font);
        }
        self.font = CreateFontW(
            (-16. * d) as i32,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        self.controls.clear();
        self.layout.clear();
        self.page_controls.clear();
        self.building_page = 255;
        for (i, text) in ["常规", "视线与角度", "自定义姿态", "Blockbench"].iter().enumerate() {
            self.control(
                "BUTTON",
                text,
                BS_RADIOBUTTON as u32 | BS_PUSHLIKE as u32 | WS_TABSTOP,
                24 + i as i32 * 178,
                16,
                170,
                34,
                150 + i as i32,
            );
        }
        self.building_page = 0;
        self.control("STATIC", "皮肤", 0, 24, 82, 70, 24, 0);
        self.control("STATIC", "", SS_PATHELLIPSIS, 100, 82, 350, 24, 300);
        self.control(
            "BUTTON",
            "导入皮肤…",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            490,
            76,
            120,
            32,
            101,
        );
        self.control(
            "BUTTON",
            "恢复内置",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            624,
            76,
            110,
            32,
            102,
        );
        self.control("STATIC", "正版 ID", 0, 24, 132, 90, 24, 331);
        let input = self.control(
            "EDIT",
            &self.online_id.clone(),
            WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            125,
            126,
            350,
            30,
            330,
        );
        SendMessageW(input, EM_SETLIMITTEXT, 36, 0);
        SendMessageW(
            input,
            0x1501,
            0,
            wide("Java 版用户名或 UUID").as_ptr() as isize,
        );
        self.control(
            "BUTTON",
            "获取皮肤",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            490,
            125,
            120,
            32,
            104,
        );
        self.control("STATIC", "无需登录", 0, 624, 132, 110, 24, 332);
        self.control(
            "STATIC",
            &self.online_notice.clone(),
            0,
            125,
            163,
            609,
            24,
            333,
        );
        self.control("STATIC", "手臂", 0, 24, 200, 65, 24, 0);
        self.radio("经典（4 px）", 100, 196, 150, 114);
        self.radio("纤细（3 px）", 260, 196, 150, 115);
        self.check("显示皮肤外层", 490, 196, 220, 124);
        self.control("STATIC", "姿势", 0, 24, 244, 65, 24, 0);
        self.radio("站立", 100, 240, 100, 110);
        self.radio("坐下", 210, 240, 100, 111);
        self.radio("自定义", 320, 240, 130, 116);
        self.control("STATIC", "站立时按住 Shift 潜行", 0, 490, 244, 244, 24, 0);
        self.control("STATIC", "敲字", 0, 24, 288, 65, 24, 0);
        self.radio("精准按键", 100, 284, 150, 112);
        self.radio("双手交替", 260, 284, 150, 113);
        self.control(
            "STATIC",
            "坐姿下生效；主键区和 F 功能键按位置敲击。",
            0,
            100,
            322,
            634,
            24,
            0,
        );
        self.control("STATIC", "主手", 0, 24, 366, 65, 24, 0);
        self.radio("右手挥动", 100, 362, 150, 118);
        self.radio("左手挥动", 260, 362, 150, 119);
        self.control("STATIC", "按角色自身的左右区分", 0, 490, 366, 244, 24, 0);
        self.control("STATIC", "桌面", 0, 24, 414, 65, 24, 0);
        self.check("保持置顶", 100, 410, 150, 120);
        self.check("完全鼠标穿透", 300, 410, 220, 121);
        self.control(
            "BUTTON",
            "找回桌宠",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            594,
            408,
            140,
            32,
            130,
        );
        self.check("全局键盘互动", 100, 452, 210, 122);
        self.check("hyp快速队伍聊天", 320, 452, 220, 137);
        self.control("STATIC", "# → /p chat", 0, 555, 456, 179, 24, 0);
        self.control("STATIC", "大小", 0, 24, 502, 65, 24, 0);
        self.slider(200, 100, 498, 540, 65, 180);
        self.control("STATIC", "", SS_RIGHT, 650, 502, 84, 24, 310);
        self.control("STATIC", "帧率", 0, 24, 550, 65, 24, 0);
        self.slider(207, 100, 546, 540, 0, 6);
        self.control("STATIC", "", SS_RIGHT, 650, 550, 84, 24, 607);
        self.control("STATIC", "打字压低", 0, 24, 588, 120, 24, 0);
        self.slider(211, 150, 584, 490, 0, 30);
        self.control("STATIC", "", SS_RIGHT, 650, 588, 84, 24, 611);
        self.check("显示正版披风", 24, 626, 210, 138);
        self.control("STATIC", "仅在账号有披风时出现", 0, 250, 628, 260, 24, 0);
        self.control("STATIC", "开机自启", 0, 24, 664, 90, 24, 0);
        self.check("随 Windows 登录自动启动", 125, 660, 250, 160);
        self.control("STATIC", "进程优先级", 0, 400, 664, 95, 24, 0);
        self.combo(
            161,
            500,
            660,
            150,
            &["空闲", "低于正常", "正常", "高于正常", "高"],
            config::PRIORITY_OPTIONS
                .iter()
                .position(|&p| p == self.settings.startup_priority)
                .unwrap_or(2),
        );
        self.control("STATIC", "启动延迟", 0, 24, 700, 90, 24, 0);
        self.combo(
            162,
            125,
            696,
            150,
            &["立即", "5 秒", "15 秒", "30 秒", "1 分钟", "2 分钟"],
            config::AUTOSTART_DELAYS
                .iter()
                .position(|&d| d == self.settings.autostart_delay)
                .unwrap_or(0),
        );
        self.control("STATIC", "延迟只对开机自启生效", 0, 295, 702, 250, 24, 0);

        self.building_page = 1;
        self.control("STATIC", "整体角度偏移", 0, 24, 78, 280, 24, 0);
        self.control(
            "BUTTON",
            "角度归零",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            624,
            72,
            110,
            30,
            133,
        );
        for (i, label) in ["前后俯仰 · X", "左右转向 · Y", "左右倾斜 · Z"]
            .iter()
            .enumerate()
        {
            let y = 122 + i as i32 * 43;
            self.control("STATIC", label, 0, 24, y, 160, 24, 0);
            self.slider(208 + i as i32, 180, y - 3, 445, -180, 180);
            self.control("STATIC", "", SS_RIGHT, 636, y, 98, 24, 608 + i as i32);
        }
        self.check("视线跟随鼠标", 24, 268, 240, 123);
        self.control(
            "BUTTON",
            "跟随复位",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            624,
            264,
            110,
            30,
            132,
        );
        self.control("STATIC", "水平 · 左右", 0, 24, 314, 240, 24, 0);
        self.control("STATIC", "垂直 · 上下", 0, 400, 314, 240, 24, 0);
        for (col, base) in [(24, 201), (400, 204)] {
            for (row, label) in ["灵敏度", "转动范围", "中心偏移"].iter().enumerate() {
                let y = 356 + row as i32 * 62;
                let id = base + row as i32;
                self.control("STATIC", label, 0, col, y, 160, 22, 0);
                self.control("STATIC", "", SS_RIGHT, col + 235, y, 95, 22, 400 + id);
                let (min, max) = match row {
                    0 => (0, 300),
                    1 => (0, 170),
                    _ => (-60, 60),
                };
                self.slider(id, col, y + 23, 330, min, max);
            }
        }
        self.control("STATIC", "负值偏左，正值偏右", 0, 24, 550, 335, 24, 0);
        self.control("STATIC", "负值抬头，正值低头", 0, 400, 550, 335, 24, 0);

        self.building_page = 2;
        self.radio("使用自定义姿态", 24, 74, 230, 117);
        self.radio("站姿", 300, 74, 110, 135);
        self.radio("坐姿", 425, 74, 110, 136);
        self.control(
            "BUTTON",
            "姿态归零",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            624,
            72,
            110,
            30,
            134,
        );
        self.control(
            "STATIC",
            "站姿和坐姿分别保存；坐姿角度在默认坐姿上调整。",
            0,
            24,
            116,
            710,
            24,
            0,
        );
        for (axis, label) in ["前后旋转 · X", "左右旋转 · Y", "侧向旋转 · Z"]
            .iter()
            .enumerate()
        {
            self.control("STATIC", label, 0, 120 + axis as i32 * 205, 164, 200, 24, 0);
        }
        for (row, name) in ["头部", "身体", "左臂", "右臂", "左腿", "右腿"]
            .iter()
            .enumerate()
        {
            let y = 214 + row as i32 * 54;
            self.control("STATIC", name, 0, 24, y, 90, 24, 0);
            for axis in 0..3 {
                let id = 220 + row as i32 * 3 + axis;
                let x = 112 + axis * 205;
                self.slider(id, x, y - 3, 135, -180, 180);
                self.control("STATIC", "", SS_RIGHT, x + 137, y, 60, 24, id + 400);
            }
        }
        self.control(
            "STATIC",
            "坐姿支持敲字、长按和挥手；互动结束回到所设姿态。",
            0,
            24,
            554,
            710,
            24,
            0,
        );
        self.control(
            "STATIC",
            "头部角度在关闭视线跟随后生效；不弯曲肢体。",
            0,
            24,
            580,
            710,
            24,
            0,
        );

        self.building_page = 3;
        self.control("STATIC", "模型模式", 0, 24, 82, 90, 24, 0);
        self.check("启用 Blockbench 3D 模型", 125, 78, 260, 500);
        self.control(
            "BUTTON",
            "导入模型…",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            490,
            76,
            120,
            32,
            501,
        );
        self.control(
            "BUTTON",
            "恢复玩家皮肤",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            624,
            76,
            110,
            32,
            502,
        );

        self.control("STATIC", "当前模型", 0, 24, 132, 90, 24, 0);
        self.control(
            "STATIC",
            "未加载任何 Blockbench 模型",
            SS_PATHELLIPSIS,
            125,
            132,
            609,
            24,
            503,
        );

        self.control("STATIC", "选择动画", 0, 24, 180, 90, 24, 0);
        self.combo(504, 125, 176, 350, &["无动画（基础姿态）"], 0);

        self.check("循环播放动画", 490, 178, 220, 505);

        self.control("STATIC", "互动效果", 0, 24, 230, 90, 24, 0);
        self.check("头部骨骼跟随鼠标视线", 125, 226, 260, 506);
        self.check("按键身体回弹压低", 490, 226, 220, 513);

        self.control("STATIC", "播放速度", 0, 24, 280, 90, 24, 0);
        self.slider(507, 125, 276, 515, 25, 300);
        self.control("STATIC", "1.00x", SS_RIGHT, 650, 280, 84, 24, 508);

        self.control("STATIC", "部件微调", 0, 24, 328, 90, 24, 0);
        self.combo(510, 125, 324, 350, &["(未加载模型)"], 0);
        self.control(
            "BUTTON",
            "当前归零",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            490,
            322,
            110,
            32,
            511,
        );
        self.control(
            "BUTTON",
            "全部归零",
            BS_PUSHBUTTON as u32 | WS_TABSTOP,
            610,
            322,
            110,
            32,
            512,
        );

        self.control("STATIC", "骨骼位置偏移 (格)", 0, 24, 370, 320, 22, 0);
        self.control("STATIC", "骨骼旋转角度 (度)", 0, 380, 370, 320, 22, 0);

        self.control("STATIC", "X 左右", 0, 24, 405, 60, 24, 0);
        self.slider(520, 86, 402, 195, -100, 100);
        self.control("STATIC", "+0.0", SS_RIGHT, 285, 405, 65, 24, 521);

        self.control("STATIC", "Y 上下", 0, 24, 450, 60, 24, 0);
        self.slider(522, 86, 447, 195, -100, 100);
        self.control("STATIC", "+0.0", SS_RIGHT, 285, 450, 65, 24, 523);

        self.control("STATIC", "Z 前后", 0, 24, 495, 60, 24, 0);
        self.slider(524, 86, 492, 195, -100, 100);
        self.control("STATIC", "+0.0", SS_RIGHT, 285, 495, 65, 24, 525);

        self.control("STATIC", "X 俯仰", 0, 380, 405, 60, 24, 0);
        self.slider(530, 442, 402, 195, -180, 180);
        self.control("STATIC", "+0°", SS_RIGHT, 641, 405, 65, 24, 531);

        self.control("STATIC", "Y 偏航", 0, 380, 450, 60, 24, 0);
        self.slider(532, 442, 447, 195, -180, 180);
        self.control("STATIC", "+0°", SS_RIGHT, 641, 450, 65, 24, 533);

        self.control("STATIC", "Z 翻滚", 0, 380, 495, 60, 24, 0);
        self.slider(534, 442, 492, 195, -180, 180);
        self.control("STATIC", "+0°", SS_RIGHT, 641, 495, 65, 24, 535);

        self.control(
            "STATIC",
            "提示：选择部件骨骼后拖动滑块即可调整位置和角度，调整即时生效并自动保存。",
            0,
            24,
            550,
            710,
            24,
            509,
        );
        self.control(
            "STATIC",
            "支持 Blockbench 导出的立方体、骨骼层级、贴图以及骨骼动画关键帧。",
            0,
            24,
            576,
            710,
            24,
            0,
        );

        self.building_page = 255;
        self.status = self.control("STATIC", "已保存", 0, 24, 738, 465, 24, 320);
        self.control(
            "STATIC",
            "由GPT 6 Astra制作",
            SS_RIGHT,
            490,
            738,
            244,
            24,
            0,
        );
        self.control(
            "STATIC",
            "拖动摆放 · 右键菜单 · 双击设置；关闭此窗口不会退出桌宠。",
            0,
            24,
            764,
            710,
            22,
            0,
        );
        self.show_page(self.panel_page);
        self.refresh_panel();
        DragAcceptFiles(self.panel, 1);
        ShowWindow(self.panel, SW_SHOW);
        SetForegroundWindow(self.panel);
    }
    pub(super) unsafe fn show_page(&mut self, page: u8) {
        self.panel_page = page.min(3);
        for &(hwnd, owner) in &self.page_controls {
            ShowWindow(
                hwnd,
                if owner == 255 || owner == self.panel_page {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
        }
        for i in 0..4 {
            SendMessageW(
                self.ctrl(150 + i),
                BM_SETCHECK,
                if i == self.panel_page as i32 {
                    BST_CHECKED as usize
                } else {
                    BST_UNCHECKED as usize
                },
                0,
            );
        }
        if !self.panel.is_null() {
            InvalidateRect(self.panel, null(), 1);
        }
    }
    unsafe fn panel_fit(&self, monitor: HMONITOR, dpi: f32) -> f32 {
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        dpi.min((info.rcWork.right - info.rcWork.left - 32) as f32 / 780.)
            .min((info.rcWork.bottom - info.rcWork.top - 64) as f32 / 796.)
    }
    unsafe fn panel_dpi(&mut self, hwnd: HWND, w: WPARAM, l: LPARAM) {
        let p = *(l as *const RECT);
        let dpi = (w & 0xffff) as f32 / 96.;
        let monitor = MonitorFromRect(&p, MONITOR_DEFAULTTONEAREST);
        let d = self.panel_fit(monitor, dpi);
        self.panel_scale = d;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (760. * d) as i32,
            bottom: (786. * d) as i32,
        };
        AdjustWindowRectExForDpi(
            &mut rect,
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            0,
            0,
            (dpi * 96.) as u32,
        );
        SetWindowPos(
            hwnd,
            null_mut(),
            p.left,
            p.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let font = CreateFontW(
            (-16. * d) as i32,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            wide("Microsoft YaHei UI").as_ptr(),
        );
        for &(h, [x, y, ww, hh]) in &self.layout {
            SendMessageW(h, WM_SETFONT, font as usize, 1);
            SetWindowPos(
                h,
                null_mut(),
                (x as f32 * d) as i32,
                (y as f32 * d) as i32,
                (ww as f32 * d) as i32,
                (hh as f32 * d) as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        DeleteObject(self.font);
        self.font = font;
        InvalidateRect(hwnd, null(), 1);
    }
    unsafe fn radio(&mut self, text: &str, x: i32, y: i32, w: i32, id: i32) {
        self.control(
            "BUTTON",
            text,
            BS_RADIOBUTTON as u32 | WS_TABSTOP,
            x,
            y,
            w,
            28,
            id,
        );
    }
    unsafe fn check(&mut self, text: &str, x: i32, y: i32, w: i32, id: i32) {
        self.control(
            "BUTTON",
            text,
            BS_CHECKBOX as u32 | WS_TABSTOP,
            x,
            y,
            w,
            28,
            id,
        );
    }
    unsafe fn combo(&mut self, id: i32, x: i32, y: i32, w: i32, items: &[&str], selected: usize) {
        let h = self.control(
            "COMBOBOX",
            "",
            CBS_DROPDOWNLIST as u32 | WS_VSCROLL | WS_TABSTOP,
            x,
            y,
            w,
            200,
            id,
        );
        for item in items {
            SendMessageW(h, CB_ADDSTRING, 0, wide(item).as_ptr() as isize);
        }
        SendMessageW(h, CB_SETCURSEL, selected, 0);
    }
    unsafe fn slider(&mut self, id: i32, x: i32, y: i32, w: i32, min: i32, max: i32) {
        let h = self.control(
            "msctls_trackbar32",
            "",
            TBS_HORZ
                | (if id == 207 {
                    TBS_AUTOTICKS
                } else {
                    TBS_NOTICKS
                })
                | WS_TABSTOP,
            x,
            y,
            w,
            29,
            id,
        );
        SendMessageW(h, TBM_SETRANGEMIN, 0, min as isize);
        SendMessageW(h, TBM_SETRANGEMAX, 1, max as isize);
        SendMessageW(h, TBM_SETPAGESIZE, 0, if id == 207 { 1 } else { 5 });
    }
    fn ctrl(&self, id: i32) -> HWND {
        self.controls
            .iter()
            .find(|p| p.0 == id)
            .map(|p| p.1)
            .unwrap_or(null_mut())
    }
    pub(super) unsafe fn refresh_panel(&mut self) {
        if self.panel.is_null() {
            return;
        }
        EnableWindow(self.ctrl(104), if self.download.is_some() { 0 } else { 1 });
        SetWindowTextW(
            self.ctrl(104),
            wide(if self.download.is_some() {
                "获取中…"
            } else {
                "获取皮肤"
            })
            .as_ptr(),
        );
        SetWindowTextW(self.ctrl(333), wide(&self.online_notice).as_ptr());
        for (id, checked) in [
            (110, self.settings.posture == "standing"),
            (111, self.settings.posture == "sitting"),
            (116, self.settings.posture == "custom"),
            (117, self.settings.posture == "custom"),
            (135, self.settings.custom_base == "standing"),
            (136, self.settings.custom_base == "sitting"),
            (118, self.settings.main_hand == "right"),
            (119, self.settings.main_hand == "left"),
            (112, self.settings.typing_mode == "precise"),
            (113, self.settings.typing_mode == "alternating"),
            (114, self.settings.arm_model == "classic"),
            (115, self.settings.arm_model == "slim"),
            (120, self.settings.always_on_top),
            (121, self.settings.click_through),
            (122, self.settings.keyboard_enabled),
            (137, self.settings.hyp_quick_chat),
            (123, self.settings.track_mouse),
            (124, self.settings.outer_layer),
            (138, self.settings.show_cape),
            (160, self.settings.autostart),
            (500, self.settings.model_mode == "blockbench"),
            (505, self.settings.blockbench_loop),
            (506, self.settings.blockbench_head_track),
            (513, self.settings.blockbench_typing_squash),
        ] {
            SendMessageW(
                self.ctrl(id),
                BM_SETCHECK,
                if checked {
                    BST_CHECKED as usize
                } else {
                    BST_UNCHECKED as usize
                },
                0,
            );
        }
        SendMessageW(
            self.ctrl(161),
            CB_SETCURSEL,
            config::PRIORITY_OPTIONS
                .iter()
                .position(|&p| p == self.settings.startup_priority)
                .unwrap_or(2),
            0,
        );
        SendMessageW(
            self.ctrl(162),
            CB_SETCURSEL,
            config::AUTOSTART_DELAYS
                .iter()
                .position(|&d| d == self.settings.autostart_delay)
                .unwrap_or(0),
            0,
        );
        SetWindowTextW(self.ctrl(300), wide(&self.settings.skin_name).as_ptr());
        let model_label = if self.settings.blockbench_name.is_empty() {
            "未加载任何 Blockbench 模型（默认使用标准玩家模型）"
        } else {
            &self.settings.blockbench_name
        };
        SetWindowTextW(self.ctrl(503), wide(model_label).as_ptr());

        let h_anim = self.ctrl(504);
        if !h_anim.is_null() {
            let mut anim_list = vec!["无动画（基础姿态）".to_string()];
            if let Some(m) = &self.bbmodel {
                for a in m.animation_names() {
                    anim_list.push(a);
                }
            }
            let cur_count = SendMessageW(h_anim, CB_GETCOUNT, 0, 0) as usize;
            if cur_count != anim_list.len() {
                SendMessageW(h_anim, CB_RESETCONTENT, 0, 0);
                for item in &anim_list {
                    SendMessageW(h_anim, CB_ADDSTRING, 0, wide(item).as_ptr() as isize);
                }
            }
            let sel_idx = if self.settings.blockbench_animation.is_empty() {
                0
            } else {
                anim_list
                    .iter()
                    .position(|a| a == &self.settings.blockbench_animation)
                    .unwrap_or(0)
            };
            SendMessageW(h_anim, CB_SETCURSEL, sel_idx, 0);
        }

        let h_bone = self.ctrl(510);
        if !h_bone.is_null() {
            let mut bone_list: Vec<String> = Vec::new();
            if let Some(m) = &self.bbmodel {
                for b in &m.bones {
                    bone_list.push(b.name.clone());
                }
            }
            if bone_list.is_empty() {
                bone_list.push("(未加载模型)".to_string());
            }
            let cur_count = SendMessageW(h_bone, CB_GETCOUNT, 0, 0) as usize;
            if cur_count != bone_list.len() {
                SendMessageW(h_bone, CB_RESETCONTENT, 0, 0);
                for item in &bone_list {
                    SendMessageW(h_bone, CB_ADDSTRING, 0, wide(item).as_ptr() as isize);
                }
            }
            if self.bbmodel.is_some() {
                if !bone_list.iter().any(|b| b == &self.selected_blockbench_bone) {
                    if let Some(first) = bone_list.first() {
                        self.selected_blockbench_bone = first.clone();
                    }
                }
            } else {
                self.selected_blockbench_bone.clear();
            }
            let sel_idx = if self.selected_blockbench_bone.is_empty() {
                0
            } else {
                bone_list
                    .iter()
                    .position(|b| b == &self.selected_blockbench_bone)
                    .unwrap_or(0)
            };
            SendMessageW(h_bone, CB_SETCURSEL, sel_idx, 0);
            let has_model = if self.bbmodel.is_some() { 1 } else { 0 };
            EnableWindow(h_bone, has_model);
            EnableWindow(self.ctrl(511), has_model);
            EnableWindow(self.ctrl(512), has_model);
        }

        let offsets = if !self.selected_blockbench_bone.is_empty() {
            self.settings
                .blockbench_part_offsets
                .get(&self.selected_blockbench_bone)
                .or_else(|| {
                    self.settings
                        .blockbench_part_offsets
                        .get(&self.selected_blockbench_bone.to_lowercase())
                })
                .copied()
                .unwrap_or([0.0; 6])
        } else {
            [0.0; 6]
        };

        let has_model = if self.bbmodel.is_some() { 1 } else { 0 };
        for (s_id, l_id, val) in [
            (520, 521, offsets[0]),
            (522, 523, offsets[1]),
            (524, 525, offsets[2]),
        ] {
            let slider_val = (val * 2.0).round() as i32;
            SendMessageW(self.ctrl(s_id), TBM_SETPOS, 1, slider_val as isize);
            SetWindowTextW(self.ctrl(l_id), wide(&format!("{:+0.1}", val)).as_ptr());
            EnableWindow(self.ctrl(s_id), has_model);
        }

        for (s_id, l_id, val) in [
            (530, 531, offsets[3]),
            (532, 533, offsets[4]),
            (534, 535, offsets[5]),
        ] {
            let slider_val = val.round() as i32;
            SendMessageW(self.ctrl(s_id), TBM_SETPOS, 1, slider_val as isize);
            SetWindowTextW(self.ctrl(l_id), wide(&format!("{:+0}°", slider_val)).as_ptr());
            EnableWindow(self.ctrl(s_id), has_model);
        }

        let values = [
            (self.settings.scale * 100.).round() as i32,
            self.settings.head_sensitivity_x as i32,
            (self.settings.head_range_x * 2.).round() as i32,
            self.settings.head_center_x as i32,
            self.settings.head_sensitivity_y as i32,
            (self.settings.head_range_y * 2.).round() as i32,
            self.settings.head_center_y as i32,
            config::FPS_OPTIONS
                .iter()
                .position(|&fps| fps == self.settings.fps)
                .unwrap_or(1) as i32,
            self.settings.typing_squash.round() as i32,
            (self.settings.blockbench_speed * 100.).round() as i32,
        ];
        for (i, &id) in SLIDER_IDS.iter().enumerate() {
            SendMessageW(self.ctrl(id), TBM_SETPOS, 1, values[i] as isize);
            let text = match id {
                200 | 201 | 204 | 211 => format!("{}%", values[i]),
                202 | 205 => format!("±{}°", values[i] as f32 / 2.),
                207 => format!("{} FPS", self.settings.fps),
                507 => format!("{:.2}x", self.settings.blockbench_speed),
                _ => format!("{:+}°", values[i]),
            };
            SetWindowTextW(
                self.ctrl(if id == 200 { 310 } else if id == 507 { 508 } else { 400 + id }),
                wide(&text).as_ptr(),
            );
        }
        for axis in 0..3 {
            let value = self.settings.model_rotation[axis] as i32;
            SendMessageW(self.ctrl(208 + axis as i32), TBM_SETPOS, 1, value as isize);
            SetWindowTextW(
                self.ctrl(608 + axis as i32),
                wide(&format!("{value:+}°")).as_ptr(),
            );
        }
        for row in 0..6 {
            for axis in 0..3 {
                let id = 220 + row as i32 * 3 + axis as i32;
                let value = self.settings.active_custom_pose().row(row)[axis] as i32;
                SendMessageW(self.ctrl(id), TBM_SETPOS, 1, value as isize);
                SetWindowTextW(self.ctrl(id + 400), wide(&format!("{value:+}°")).as_ptr());
            }
        }
        SetWindowTextW(self.status, wide(&self.notice).as_ptr());
    }
    unsafe fn slide(&mut self, hwnd: HWND, tracking: bool) {
        let id = GetDlgCtrlID(hwnd);
        let value = SendMessageW(hwnd, TBM_GETPOS, 0, 0) as f32;
        match id {
            200 => {
                self.settings.scale = value / 100.;
                self.resize_pet();
            }
            201 => self.settings.head_sensitivity_x = value,
            202 => self.settings.head_range_x = value / 2.,
            203 => self.settings.head_center_x = value,
            204 => self.settings.head_sensitivity_y = value,
            205 => self.settings.head_range_y = value / 2.,
            206 => self.settings.head_center_y = value,
            207 => {
                self.settings.fps = config::FPS_OPTIONS[(value as usize).min(6)];
                self.next_draw = self.start.elapsed().as_secs_f64();
            }
            208..=210 => self.settings.model_rotation[(id - 208) as usize] = value,
            211 => self.settings.typing_squash = value,
            507 => self.settings.blockbench_speed = value / 100.,
            520 | 522 | 524 => {
                if !self.selected_blockbench_bone.is_empty() {
                    let axis = match id {
                        520 => 0,
                        522 => 1,
                        _ => 2,
                    };
                    let entry = self
                        .settings
                        .blockbench_part_offsets
                        .entry(self.selected_blockbench_bone.clone())
                        .or_insert([0.0; 6]);
                    entry[axis] = value * 0.5;
                }
            }
            530 | 532 | 534 => {
                if !self.selected_blockbench_bone.is_empty() {
                    let axis = match id {
                        530 => 3,
                        532 => 4,
                        _ => 5,
                    };
                    let entry = self
                        .settings
                        .blockbench_part_offsets
                        .entry(self.selected_blockbench_bone.clone())
                        .or_insert([0.0; 6]);
                    entry[axis] = value;
                }
            }
            220..=237 => {
                self.settings
                    .active_custom_pose_mut()
                    .row_mut((id - 220) as usize / 3)[(id - 220) as usize % 3] = value;
                if self.settings.posture != "custom" {
                    self.reset_keys();
                    self.pose.reset_swing();
                    self.settings.posture = "custom".into();
                    self.pose.sit = if self.settings.is_sitting() { 1. } else { 0. };
                }
            }
            _ => return,
        }
        self.settings.clean();
        self.wake();
        let text = match id {
            200 | 201 | 204 | 211 => format!("{}%", value as i32),
            202 | 205 => format!("±{}°", value / 2.),
            207 => format!("{} FPS", self.settings.fps),
            507 => format!("{:.2}x", self.settings.blockbench_speed),
            520 | 522 | 524 => format!("{:+0.1}", value * 0.5),
            530 | 532 | 534 => format!("{:+0}°", value as i32),
            _ => format!("{:+}°", value as i32),
        };
        let label_ctrl = if id == 200 {
            self.ctrl(310)
        } else if id == 507 {
            self.ctrl(508)
        } else if (520..=525).contains(&id) || (530..=535).contains(&id) {
            self.ctrl(id + 1)
        } else {
            self.ctrl(id + 400)
        };
        SetWindowTextW(label_ctrl, wide(&text).as_ptr());
        if tracking {
            if !self.slider_pending {
                SetWindowTextW(self.status, wide("调整中，松手保存").as_ptr());
            }
            self.slider_pending = true;
        } else {
            self.save();
            self.refresh_panel();
        }
    }
    pub(super) unsafe fn combo_changed(&mut self, id: u32) {
        let index = SendMessageW(self.ctrl(id as i32), CB_GETCURSEL, 0, 0) as usize;
        match id {
            161 => {
                let Some(name) = config::PRIORITY_OPTIONS.get(index) else {
                    return;
                };
                self.settings.startup_priority = (*name).into();
                self.settings.clean();
                self.notice = match autostart::apply_priority(name) {
                    Ok(label) => format!("进程优先级已设为{label}"),
                    Err(e) => e,
                };
            }
            162 => {
                let Some(delay) = config::AUTOSTART_DELAYS.get(index) else {
                    return;
                };
                self.settings.autostart_delay = *delay;
                self.settings.clean();
                self.notice = if *delay == 0 {
                    "开机自启将立即启动".into()
                } else {
                    format!("开机自启将延迟 {delay} 秒")
                };
            }
            504 => {
                if index == 0 {
                    self.settings.blockbench_animation.clear();
                    self.notice = "已切换为基础姿态（无动画）".into();
                } else if let Some(m) = &self.bbmodel {
                    let names = m.animation_names();
                    if let Some(name) = names.get(index - 1) {
                        self.settings.blockbench_animation = name.clone();
                        self.notice = format!("正在播放动画: {name}");
                    }
                }
                self.changed();
            }
            510 => {
                if let Some(m) = &self.bbmodel {
                    if let Some(b) = m.bones.get(index) {
                        self.selected_blockbench_bone = b.name.clone();
                    }
                }
            }
            _ => return,
        }
        self.save();
        self.refresh_panel();
    }
    pub(super) unsafe fn fetch_skin(&mut self) {
        if self.download.is_some() {
            return;
        }
        let mut text = [0u16; 64];
        let len = GetWindowTextW(self.ctrl(330), text.as_mut_ptr(), 64);
        let id = String::from_utf16_lossy(&text[..len as usize])
            .trim()
            .to_owned();
        if let Err(e) = online::identity(&id) {
            self.online_notice = e;
            self.refresh_panel();
            return;
        }
        self.online_id = id.clone();
        self.online_notice = "正在连接官方皮肤服务…".into();
        let (tx, rx) = std::sync::mpsc::channel();
        self.download = Some(rx);
        match std::thread::Builder::new()
            .name("skin-download".into())
            .spawn(move || {
                let _ = tx.send(online::fetch(&id));
            }) {
            Ok(_) => {}
            Err(e) => {
                self.download = None;
                self.online_notice = format!("无法开始下载：{e}");
            }
        }
        self.refresh_panel();
    }
    pub(super) unsafe fn poll_download(&mut self) {
        let result = match self.download.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                Err("下载已中断，请重试。".into())
            }
            _ => return,
        };
        self.download = None;
        match result {
            Ok(result) => {
                let dest = self.store.dir.join("skin.png");
                if let Err(e) = config::atomic(&dest, &result.bytes) {
                    self.online_notice = format!("皮肤保存失败：{e}");
                    self.refresh_panel();
                    return;
                }
                self.skin = result.skin;
                self.settings.skin_path = dest.to_string_lossy().into_owned();
                // 正版披风：拿到就存成 cape.png，没有就清掉旧披风。
                let mut cape_notice = String::new();
                match result.cape {
                    Some(bytes) => {
                        let cape_dest = self.store.dir.join("cape.png");
                        if let Ok(cape) = Cape::load(&bytes) {
                            if config::atomic(&cape_dest, &bytes).is_ok() {
                                self.settings.cape_path = cape_dest.to_string_lossy().into_owned();
                                self.cape = Some(cape);
                                cape_notice = "，含披风".into();
                            }
                        }
                    }
                    None => {
                        self.settings.cape_path.clear();
                        self.cape = None;
                        cape_notice = "，该账号没有披风".into();
                    }
                }
                self.settings.skin_name = format!("{} · 正版皮肤", result.name);
                self.settings.arm_model = if result.slim && !self.skin.legacy {
                    "slim"
                } else {
                    "classic"
                }
                .into();
                self.changed();
                self.online_notice =
                    format!("已获取 {} 的皮肤{cape_notice}，已自动识别手臂类型。", result.name);
            }
            Err(e) => self.online_notice = e,
        }
        self.refresh_panel();
    }
    pub(super) unsafe fn choose_skin(&mut self) {
        let mut buffer = [0u16; 32768];
        let filter = wide("Minecraft PNG 皮肤\0*.png\0\0");
        let mut data: OPENFILENAMEW = std::mem::zeroed();
        data.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        data.hwndOwner = if self.panel.is_null() {
            self.hwnd
        } else {
            self.panel
        };
        data.lpstrFilter = filter.as_ptr();
        data.lpstrFile = buffer.as_mut_ptr();
        data.nMaxFile = buffer.len() as u32;
        let title = wide("选择 Minecraft 皮肤");
        data.lpstrTitle = title.as_ptr();
        data.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut data) != 0 {
            let len = buffer.iter().position(|&c| c == 0).unwrap();
            self.import(PathBuf::from(String::from_utf16_lossy(&buffer[..len])));
        }
    }
    pub(super) unsafe fn choose_model(&mut self) {
        let mut buffer = [0u16; 32768];
        let filter = wide("Blockbench 模型 (*.bbmodel;*.geo.json)\0*.bbmodel;*.geo.json;*.json\0所有文件 (*.*)\0*.*\0\0");
        let mut data: OPENFILENAMEW = std::mem::zeroed();
        data.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        data.hwndOwner = if self.panel.is_null() {
            self.hwnd
        } else {
            self.panel
        };
        data.lpstrFilter = filter.as_ptr();
        data.lpstrFile = buffer.as_mut_ptr();
        data.nMaxFile = buffer.len() as u32;
        let title = wide("选择 Blockbench 3D 模型");
        data.lpstrTitle = title.as_ptr();
        data.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut data) != 0 {
            let len = buffer.iter().position(|&c| c == 0).unwrap();
            self.import(PathBuf::from(String::from_utf16_lossy(&buffer[..len])));
        }
    }
}
pub(super) unsafe fn panel_proc(
    a: &mut App,
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (w & 0xffff) as u32;
            if ((w >> 16) & 0xffff) as u32 == CBN_SELCHANGE {
                a.combo_changed(id);
            } else {
                a.command(id);
            }
            0
        }
        WM_HSCROLL => {
            if l != 0 {
                a.slide(l as HWND, (w & 0xffff) as u32 == TB_THUMBTRACK);
            }
            0
        }
        WM_DROPFILES => {
            drop_skin(a, w);
            0
        }
        WM_CTLCOLORSTATIC => {
            let dc = w as HDC;
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0x00252525);
            GetStockObject(WHITE_BRUSH) as isize
        }
        WM_CTLCOLORBTN => GetStockObject(WHITE_BRUSH) as isize,
        WM_QUERYENDSESSION => {
            a.save();
            1
        }
        WM_ENDSESSION => {
            if w != 0 {
                a.save();
            }
            0
        }
        WM_DPICHANGED => {
            a.panel_dpi(hwnd, w, l);
            0
        }
        WM_CLOSE => {
            a.save();
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            a.panel = null_mut();
            a.status = null_mut();
            a.controls.clear();
            a.layout.clear();
            a.page_controls.clear();
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
pub(super) unsafe fn drop_skin(a: &mut App, w: WPARAM) {
    let drop = w as HDROP;
    let len = DragQueryFileW(drop, 0, null_mut(), 0);
    let mut name = vec![0u16; len as usize + 1];
    DragQueryFileW(drop, 0, name.as_mut_ptr(), len + 1);
    DragFinish(drop);
    if len > 0 {
        a.import(PathBuf::from(String::from_utf16_lossy(
            &name[..len as usize],
        )));
    }
}
pub(super) unsafe fn diagnostic_init(a: &mut App) {
    a.show_panel();
    diagnostic_snapshot(a);
}
pub(super) unsafe fn diagnostic_snapshot(a: &mut App) {
    let _ = config::atomic(
        &a.store.dir.join("input-trace.json"),
        serde_json::to_string_pretty(&a.input_trace)
            .unwrap()
            .as_bytes(),
    );
    let _ = config::atomic(
        &a.store.dir.join("appearance.json"),
        serde_json::to_vec(&serde_json::json!({"opacity":a.opacity}))
            .unwrap()
            .as_slice(),
    );
    let controls: Vec<_> = a
        .controls
        .iter()
        .map(|(id, h)| serde_json::json!({"id":id,"hwnd":*h as usize}))
        .collect();
    let data = serde_json::json!({"pid":std::process::id(),"pet":a.hwnd as usize,"panel":a.panel as usize,"controls":controls,"settings":a.settings,"page":a.panel_page,"renderCount":a.render_count,"rasterUs":a.raster_us,"presentUs":a.present_us,"dpi":a.dpi,"head":{"yaw":a.pose.yaw,"pitch":a.pose.pitch,"local":a.pose.head_rotation.to_array()},"sit":a.pose.sit,"crouch":a.pose.crouch,"arms":a.pose.arms.map(|q|q.to_array()),"hook":!a.hook.is_null(),"mouseHook":!a.mouse_hook.is_null(),"heldKeys":a.pose.held_codes(),"squash":a.pose.squash,"swingProgress":a.pose.swing_progress,"bodyYaw":a.pose.body_yaw,"onlinePending":a.download.is_some(),"onlineNotice":a.online_notice,"bounds":a.frame.bounds,"position":[a.pos.x,a.pos.y],"size":[a.width,a.height]});
    let _ = config::atomic(
        &a.store.dir.join("diagnostic.json"),
        serde_json::to_string_pretty(&data).unwrap().as_bytes(),
    );
    let _ = image::save_buffer(
        a.store.dir.join("pet.png"),
        &a.frame.rgba(),
        a.frame.width as u32,
        a.frame.height as u32,
        image::ColorType::Rgba8,
    );
}
