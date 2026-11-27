use crate::{input_stats::{Range, Store}, wide};
use std::{cell::Cell, collections::BTreeMap, ptr::{null, null_mut}, rc::Rc};
use windows_sys::Win32::{
    Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
};

static mut WINDOW: HWND = null_mut();
const RANGE_FIRST: usize = 400;
const TOGGLE: usize = 405;
const CLEAR: usize = 406;
const CLOSE: usize = 407;

struct WindowState {
    store: *mut Store,
    toggle: unsafe fn(bool) -> bool,
    range: usize,
    scale: f32,
    font: HFONT,
    released: Rc<Cell<bool>>,
}

pub unsafe fn open(parent: HWND, store: *mut Store, toggle: unsafe fn(bool) -> bool) {
    if !WINDOW.is_null() && IsWindow(WINDOW) != 0 {
        ShowWindow(WINDOW, SW_RESTORE);
        SetForegroundWindow(WINDOW);
        return;
    }
    let instance = GetModuleHandleW(null());
    let class = wide("MinecraftPetInputStatistics");
    let wc = WNDCLASSW { hInstance: instance, lpszClassName: class.as_ptr(),
        lpfnWndProc: Some(window_proc), hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: GetStockObject(WHITE_BRUSH) as HBRUSH, ..std::mem::zeroed() };
    RegisterClassW(&wc);
    let mut monitor: MONITORINFO = std::mem::zeroed();
    monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(MonitorFromWindow(parent, MONITOR_DEFAULTTONEAREST), &mut monitor);
    let dpi = GetDpiForWindow(parent).max(96) as f32 / 96.;
    let scale = dpi.min((monitor.rcWork.right-monitor.rcWork.left-24) as f32/940.)
        .min((monitor.rcWork.bottom-monitor.rcWork.top-60) as f32/610.).max(0.5);
    let font = CreateFontW((-14. * scale) as i32, 0, 0, 0, FW_NORMAL as i32, 0, 0, 0,
        DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, 0, wide("Microsoft YaHei UI").as_ptr());
    let released=Rc::new(Cell::new(false));
    let state = Box::new(WindowState { store, toggle, range: 0, scale, font, released: released.clone() });
    let raw = Box::into_raw(state);
    let hwnd = CreateWindowExW(0, class.as_ptr(), wide("Minecraft · 按键统计").as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        CW_USEDEFAULT, CW_USEDEFAULT, (940. * scale) as i32, (610. * scale) as i32,
        parent, null_mut(), instance, raw as *const _);
    if hwnd.is_null() {
        if !released.get() {
            let state=Box::from_raw(raw);
            if !state.font.is_null() {DeleteObject(state.font as HGDIOBJ);}
        }
        return;
    }
    WINDOW = hwnd;
    for (i, label) in ["今日", "近5小时", "本周", "本月", "累计"].iter().enumerate() {
        button(hwnd, font, label, RANGE_FIRST+i, 22+i as i32*98, 42, 90, 30, scale);
    }
    button(hwnd,font,if (*store).enabled {"暂停记录"} else {"继续记录"},TOGGLE,22,505,110,30,scale);
    button(hwnd,font,"清空统计",CLEAR,144,505,110,30,scale);
    button(hwnd,font,"关闭",CLOSE,784,505,110,30,scale);
    SetTimer(hwnd, 1, 1000, None);
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

pub unsafe fn close() {
    if !WINDOW.is_null() { DestroyWindow(WINDOW); }
}

unsafe fn button(parent: HWND, font: HFONT, label: &str, id: usize, x: i32, y: i32, w: i32, h: i32, s: f32) {
    let control=CreateWindowExW(0,wide("BUTTON").as_ptr(),wide(label).as_ptr(),
        WS_CHILD|WS_VISIBLE|WS_TABSTOP|BS_PUSHBUTTON as u32,
        (x as f32*s) as i32,(y as f32*s) as i32,(w as f32*s) as i32,(h as f32*s) as i32,
        parent,id as HMENU,GetModuleHandleW(null()),null());
    SendMessageW(control,WM_SETFONT,font as usize,1);
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
    if ptr.is_null() { return DefWindowProcW(hwnd,msg,w,l); }
    let state=&mut *ptr;
    match msg {
        WM_COMMAND => {
            let id=w&0xffff;
            if (RANGE_FIRST..RANGE_FIRST+5).contains(&id) { state.range=id-RANGE_FIRST; }
            else if id==TOGGLE {
                let requested=!(*state.store).enabled;
                if (state.toggle)(requested)!=requested {
                    MessageBoxW(hwnd,wide("无法切换统计状态，请检查输入钩子和数据文件。").as_ptr(),wide("按键统计").as_ptr(),MB_OK|MB_ICONWARNING);
                }
                SetWindowTextW(GetDlgItem(hwnd,TOGGLE as i32),wide(if (*state.store).enabled {"暂停记录"} else {"继续记录"}).as_ptr());
            } else if id==CLEAR {
                if MessageBoxW(hwnd,wide("确定清空本机所有按键和鼠标统计吗？此操作无法撤销。").as_ptr(),wide("清空统计").as_ptr(),MB_YESNO|MB_ICONQUESTION)==IDYES {
                    (*state.store).clear();
                    if let Err(error)=(*state.store).save() {
                        MessageBoxW(hwnd,wide(&error).as_ptr(),wide("统计保存失败").as_ptr(),MB_OK|MB_ICONERROR);
                    }
                }
            } else if id==CLOSE { DestroyWindow(hwnd); return 0; }
            InvalidateRect(hwnd,null(),1); 0
        }
        WM_TIMER => { InvalidateRect(hwnd,null(),0); 0 }
        WM_PAINT => { paint(hwnd,state); 0 }
        WM_CLOSE => { DestroyWindow(hwnd); 0 }
        WM_NCDESTROY => {
            KillTimer(hwnd,1); WINDOW=null_mut(); SetWindowLongPtrW(hwnd,GWLP_USERDATA,0);
            if !state.font.is_null() { DeleteObject(state.font as HGDIOBJ); }
            state.released.set(true);
            drop(Box::from_raw(ptr)); DefWindowProcW(hwnd,msg,w,l)
        }
        _ => DefWindowProcW(hwnd,msg,w,l),
    }
}

fn rgb(r:u32,g:u32,b:u32)->u32 { r|(g<<8)|(b<<16) }
fn heat(n:u64,max:u64)->u32 {
    let ratio=if max==0 {0.} else {n as f64/max as f64};
    let mix=|a:f64,b:f64| (a+(b-a)*ratio*0.85) as u32;
    rgb(mix(238.,35.),mix(244.,109.),mix(238.,65.))
}
unsafe fn rect(s:f32,x:i32,y:i32,w:i32,h:i32)->RECT {
    RECT {left:(x as f32*s) as i32,top:(y as f32*s) as i32,right:((x+w) as f32*s) as i32,bottom:((y+h) as f32*s) as i32}
}
unsafe fn text(dc:HDC,s:f32,label:&str,x:i32,y:i32,w:i32,h:i32,center:bool) {
    let mut area=rect(s,x,y,w,h);let value=wide(label);
    DrawTextW(dc,value.as_ptr(),-1,&mut area,DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX|if center {DT_CENTER} else {DT_LEFT});
}
unsafe fn key(dc:HDC,s:f32,id:&str,label:&str,keys:&BTreeMap<String,u64>,max:u64,x:i32,y:i32,w:i32,h:i32) {
    let n=*keys.get(id).unwrap_or(&0);let area=rect(s,x,y,w,h);
    let fill=CreateSolidBrush(heat(n,max));FillRect(dc,&area,fill);DeleteObject(fill as HGDIOBJ);
    FrameRect(dc,&area,GetStockObject(GRAY_BRUSH) as HBRUSH);
    SetTextColor(dc,if max>0 && n as f64/max as f64>0.6 {rgb(255,255,255)} else {rgb(30,45,33)});
    text(dc,s,label,x+1,y+2,w-2,h/2-1,true);
    if n>0 {text(dc,s,&n.to_string(),x+1,y+h/2,w-2,h/2-1,true);}
}

unsafe fn paint(hwnd: HWND, state: &WindowState) {
    let mut ps:PAINTSTRUCT=std::mem::zeroed();let dc=BeginPaint(hwnd,&mut ps);
    let previous=SelectObject(dc,state.font as HGDIOBJ);SetBkMode(dc,TRANSPARENT as i32);
    let ranges=[Range::Today,Range::Last5Hours,Range::Week,Range::Month,Range::AllTime];
    let range=state.range.min(4);let period=(*state.store).period(ranges[range]);let keys=&period.keys;let s=state.scale;
    SetTextColor(dc,rgb(28,55,37));
    text(dc,s,"键盘与鼠标统计 · 只保存在本机",22,8,620,26,false);
    text(dc,s,&format!("{} · 合计 {} 次",["今日","近5小时","本周","本月","累计"][range],period.total),22,82,650,27,false);
    text(dc,s,"键盘热力图",22,120,250,25,false);
    text(dc,s,"鼠标热力图",704,120,190,25,true);
    let keyboard_max=keys.iter().filter(|(id,_)|!id.starts_with("Mouse")).map(|(_,n)|*n).max().unwrap_or(0);
    let rows: [&[&str];6]=[
        &["Esc","F1","F2","F3","F4","F5","F6","F7","F8","F9","F10","F11","F12"],
        &["`","1","2","3","4","5","6","7","8","9","0","-","=","Backspace"],
        &["Tab","Q","W","E","R","T","Y","U","I","O","P","[","]","\\"],
        &["Caps","A","S","D","F","G","H","J","K","L",";","'","Enter"],
        &["LShift","Z","X","C","V","B","N","M",",",".","/","RShift"],
        &["LCtrl","LWin","LAlt","Space","RAlt","RWin","Menu","RCtrl"],
    ];
    for (row,items) in rows.iter().enumerate() {
        let cell=660/items.len() as i32;
        for (i,id) in items.iter().enumerate() {
            let label=match *id {"Backspace"=>"Bksp","LShift"=>"LShift","RShift"=>"RShift",_=>id};
            key(dc,s,id,label,keys,keyboard_max,22+i as i32*cell,154+row as i32*43,cell-3,39);
        }
    }
    let mouse=["MouseLeft","MouseRight","MouseMiddle","MouseSide1","MouseSide2","MouseWheelUp","MouseWheelDown"];
    let mouse_max=mouse.iter().map(|id|*keys.get(*id).unwrap_or(&0)).max().unwrap_or(0);
    let body=rect(s,758,161,133,206);let bg=CreateSolidBrush(rgb(210,222,211));
    let old_brush=SelectObject(dc,bg as HGDIOBJ);RoundRect(dc,body.left,body.top,body.right,body.bottom,(52.*s) as i32,(52.*s) as i32);
    SelectObject(dc,old_brush);DeleteObject(bg as HGDIOBJ);
    let positions=[(764,186,38,92),(844,186,38,92),(804,205,38,30),(711,204,43,42),(711,249,43,42),(804,174,38,30),(804,236,38,30)];
    let labels=["左键","右键","中键","侧键1","侧键2","上滚","下滚"];
    for i in 0..mouse.len() {let (x,y,w,h)=positions[i];key(dc,s,mouse[i],labels[i],keys,mouse_max,x,y,w,h);}
    SetTextColor(dc,rgb(35,55,39));
    text(dc,s,"最常使用 · 颜色越深代表使用越多 · 滚轮按事件计数",22,391,870,20,false);
    let mut top:Vec<_>=keys.iter().collect();
    top.sort_by(|a,b|b.1.cmp(a.1).then_with(||a.0.cmp(b.0)));
    for (i,(id,count)) in top.into_iter().take(8).enumerate() {
        let x=22+(i/4) as i32*440;let y=413+(i%4) as i32*17;
        let label=match id.as_str() {
            "MouseLeft"=>"鼠标左键","MouseRight"=>"鼠标右键","MouseMiddle"=>"鼠标中键",
            "MouseSide1"=>"鼠标侧键 1","MouseSide2"=>"鼠标侧键 2",
            "MouseWheelUp"=>"滚轮向上","MouseWheelDown"=>"滚轮向下",_=>id.as_str(),
        };
        text(dc,s,label,x,y,210,17,false);
        text(dc,s,&format!("{count} 次"),x+210,y,200,17,false);
    }
    if let Some(error)=&(*state.store).read_error {text(dc,s,&format!("统计读取失败：{error}"),22,483,870,17,false);}
    SelectObject(dc,previous);EndPaint(hwnd,&ps);
}
