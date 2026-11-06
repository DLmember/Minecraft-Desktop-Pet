use super::*;
include!("cape_catalog.rs");
const PAGE: usize = 12;
pub fn load(id: &str) -> Option<Cape> {
    ENTRIES.iter().find(|e|e.id==id).and_then(|e|Cape::load(e.bytes).ok())
}
impl App {
    pub(super) unsafe fn show_capes(&mut self) {
        if !self.cape_panel.is_null() { ShowWindow(self.cape_panel,SW_RESTORE); SetForegroundWindow(self.cape_panel); return; }
        let name=wide("MinecraftPetCapePicker");
        let mut wc:WNDCLASSEXW=std::mem::zeroed();wc.cbSize=std::mem::size_of_val(&wc) as u32;
        wc.lpfnWndProc=Some(super::wndproc);wc.hInstance=GetModuleHandleW(null());wc.lpszClassName=name.as_ptr();
        wc.hbrBackground=GetStockObject(WHITE_BRUSH) as HBRUSH;wc.hCursor=LoadCursorW(null_mut(),IDC_ARROW);wc.hIcon=self.icon;
        RegisterClassExW(&wc);
        let d=self.panel_scale;
        self.cape_panel=CreateWindowExW(0,name.as_ptr(),wide("自选披风 · Minecraft桌宠").as_ptr(),
            WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU|WS_MINIMIZEBOX,CW_USEDEFAULT,CW_USEDEFAULT,
            (660.*d) as i32,(605.*d) as i32,self.panel,null_mut(),wc.hInstance,null());
        for i in 0..PAGE {
            self.cape_control("",BS_OWNERDRAW as u32,16+(i%4) as i32*156,16+(i/4) as i32*156,148,146,700+i as i32);
        }
        self.cape_control("上一页",BS_PUSHBUTTON as u32,16,500,110,30,720);
        self.cape_control("下一页",BS_PUSHBUTTON as u32,516,500,110,30,721);
        self.cape_control("",BS_PUSHBUTTON as u32,150,500,340,30,722);
        self.refresh_capes(); ShowWindow(self.cape_panel,SW_SHOW);SetForegroundWindow(self.cape_panel);
    }
    unsafe fn cape_control(&self,text:&str,style:u32,x:i32,y:i32,w:i32,h:i32,id:i32) {
        let d=self.panel_scale;
        let c=CreateWindowExW(0,wide("BUTTON").as_ptr(),wide(text).as_ptr(),WS_CHILD|WS_VISIBLE|WS_TABSTOP|style,
            (x as f32*d) as i32,(y as f32*d) as i32,(w as f32*d) as i32,(h as f32*d) as i32,self.cape_panel,id as usize as HMENU,GetModuleHandleW(null()),null());
        SendMessageW(c,WM_SETFONT,self.font as usize,1);
    }
    pub(super) unsafe fn refresh_capes(&self) {
        if self.cape_panel.is_null(){return;}
        let dark=self.panel_dark as i32;
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(self.cape_panel,20,&dark as *const _ as *const _,4);
        for i in 0..PAGE {
            let index=self.cape_page*PAGE+i;
            EnableWindow(GetDlgItem(self.cape_panel,700+i as i32),if index<ENTRIES.len()+2 {1}else{0});
        }
        EnableWindow(GetDlgItem(self.cape_panel,720),if self.cape_page>0 {1}else{0});
        EnableWindow(GetDlgItem(self.cape_panel,721),if (self.cape_page+1)*PAGE<ENTRIES.len()+2 {1}else{0});
        let selected=match self.settings.cape_choice.as_str(){"account"=>"跟随皮肤原有披风","none"=>"无披风",id=>ENTRIES.iter().find(|e|e.id==id).map(|e|e.name).unwrap_or("未选择")};
        SetWindowTextW(GetDlgItem(self.cape_panel,722),wide(&format!("{}/{} · {}",self.cape_page+1,(ENTRIES.len()+2).div_ceil(PAGE),selected)).as_ptr());
        RedrawWindow(self.cape_panel,null(),null_mut(),RDW_INVALIDATE|RDW_ERASE|RDW_ALLCHILDREN);
    }
}
pub(super) unsafe fn panel_proc(a:&mut App,hwnd:HWND,msg:u32,w:WPARAM,l:LPARAM)->LRESULT {
    if let Some(result)=a.theme_message(hwnd,msg,w,l){
        if msg==WM_SETTINGCHANGE || msg==WM_THEMECHANGED {a.refresh_capes();}
        return result;
    }
    match msg {
        WM_COMMAND => {
            match (w&0xffff) as usize {
                720 => a.cape_page=a.cape_page.saturating_sub(1),
                721 => {if (a.cape_page+1)*PAGE<ENTRIES.len()+2 {a.cape_page+=1;}},
                id @ 700..=711 => {
                    let index=a.cape_page*PAGE+id-700;
                    if index<ENTRIES.len()+2 {
                        a.settings.cape_choice=match index {0=>"account",1=>"none",_=>ENTRIES[index-2].id}.into();
                        a.settings.show_cape=true;a.cape=load_cape(&a.settings);a.changed();
                    }
                }
                _=>{}
            }
            a.refresh_capes();0
        }
        WM_DRAWITEM => {
            let draw=&*(l as *const DRAWITEMSTRUCT);let i=a.cape_page*PAGE+draw.CtlID as usize-700;
            let bg=if a.panel_dark {0x00323232}else{0x00f4f4f4};SetDCBrushColor(draw.hDC,bg);
            FillRect(draw.hDC,&draw.rcItem,GetStockObject(DC_BRUSH) as HBRUSH);
            if i>=ENTRIES.len()+2 {return 1;}
            let (id,label)=match i{0=>("account","跟随皮肤原有披风"),1=>("none","无披风"),_=>(ENTRIES[i-2].id,ENTRIES[i-2].name)};
            let selected=a.settings.cape_choice==id;
            SetDCBrushColor(draw.hDC,if selected {0x00eab97d}else{0x00777777});
            FrameRect(draw.hDC,&draw.rcItem,GetStockObject(DC_BRUSH) as HBRUSH);
            if i>=2 {
                let thumbs=thumbnails();let pixels=&thumbs[i-2];
                let mut info:BITMAPINFO=std::mem::zeroed();info.bmiHeader.biSize=40;info.bmiHeader.biWidth=10;info.bmiHeader.biHeight=-16;info.bmiHeader.biPlanes=1;info.bmiHeader.biBitCount=32;
                let d=a.panel_scale;let width=(50.*d) as i32;let height=(80.*d) as i32;
                SetStretchBltMode(draw.hDC,COLORONCOLOR);
                StretchDIBits(draw.hDC,(draw.rcItem.right-width)/2,(12.*d) as i32,width,height,0,0,10,16,pixels.as_ptr() as *const _,&info,DIB_RGB_COLORS,SRCCOPY);
            }
            let mut rect=draw.rcItem;rect.top+=if i>=2 {(98.*a.panel_scale) as i32}else{(40.*a.panel_scale) as i32};rect.left+=4;rect.right-=4;
            SetTextColor(draw.hDC,if a.panel_dark {0x00eeeeee}else{0x00222222});SetBkMode(draw.hDC,TRANSPARENT as i32);
            SelectObject(draw.hDC,a.font);let text=wide(label);DrawTextW(draw.hDC,text.as_ptr(),-1,&mut rect,DT_CENTER|DT_WORDBREAK);1
        }
        WM_CLOSE=>{DestroyWindow(hwnd);0}
        WM_DESTROY=>{a.cape_panel=null_mut();0}
        _=>DefWindowProcW(hwnd,msg,w,l)
    }
}
fn thumbnails()-> &'static Vec<Vec<u8>> {
    static THUMBS:std::sync::OnceLock<Vec<Vec<u8>>>=std::sync::OnceLock::new();
    THUMBS.get_or_init(||ENTRIES.iter().map(|e|{
        let image=image::load_from_memory(e.bytes).unwrap().to_rgba8();let mut out=Vec::with_capacity(640);
        for y in 1..17 {for x in 1..11 {let p=image.get_pixel(x,y);out.extend([p[2],p[1],p[0],255]);}}
        out
    }).collect())
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn all_embedded_capes_are_valid_and_unique() {
        let mut ids=std::collections::HashSet::new();
        for e in ENTRIES {assert!(ids.insert(e.id));assert!(Cape::load(e.bytes).is_ok(),"{}",e.id);}
        assert_eq!(thumbnails().len(),ENTRIES.len());
    }
}
