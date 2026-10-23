use super::*;
use windows_sys::Win32::System::Registry::*;
const BACK:u32=0x00222222;
const FIELD:u32=0x00323232;
const TEXT:u32=0x00EEEEEE;
const MUTED:u32=0x00979797;
const ACCENT:u32=0x00EAB97D;

pub(super) unsafe fn system_dark()->bool {
    let mut value=1u32;let mut len=4u32;
    RegGetValueW(HKEY_CURRENT_USER,wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),wide("AppsUseLightTheme").as_ptr(),RRF_RT_REG_DWORD,null_mut(),&mut value as *mut _ as *mut _,&mut len)==0 && value==0
}
unsafe fn fill(dc:HDC,rect:&RECT,color:u32) {
    SetDCBrushColor(dc,color);FillRect(dc,rect,GetStockObject(DC_BRUSH) as HBRUSH);
}
unsafe fn border(dc:HDC,rect:&RECT,color:u32) {
    SetDCBrushColor(dc,color);FrameRect(dc,rect,GetStockObject(DC_BRUSH) as HBRUSH);
}
impl App {
    pub(super) unsafe fn apply_theme(&mut self) {
        self.panel_dark=self.theme_override.unwrap_or_else(||system_dark());
        if self.panel.is_null(){return;}
        let dark=self.panel_dark as i32;
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(self.panel,20,&dark as *const _ as *const _,4);
        self.github_icon();
        RedrawWindow(self.panel,null(),null_mut(),RDW_INVALIDATE|RDW_ERASE|RDW_ALLCHILDREN);
    }
    pub(super) unsafe fn theme_message(&mut self,hwnd:HWND,msg:u32,w:WPARAM,l:LPARAM)->Option<LRESULT> {
        if msg==WM_SETTINGCHANGE || msg==WM_THEMECHANGED {self.apply_theme();return Some(0);}
        if !self.panel_dark {return None;}
        match msg {
            WM_ERASEBKGND=>{let mut rc:RECT=std::mem::zeroed();GetClientRect(hwnd,&mut rc);fill(w as HDC,&rc,BACK);Some(1)}
            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX=>{
                let dc=w as HDC;let color=if msg==WM_CTLCOLOREDIT {FIELD}else{BACK};
                SetTextColor(dc,if IsWindowEnabled(l as HWND)!=0 {TEXT}else{MUTED});SetBkColor(dc,color);SetBkMode(dc,TRANSPARENT as i32);
                SetDCBrushColor(dc,color);Some(GetStockObject(DC_BRUSH) as isize)
            }
            WM_NOTIFY=>self.dark_draw(l),
            _=>None,
        }
    }
    unsafe fn dark_draw(&self,l:LPARAM)->Option<LRESULT> {
        if l==0{return None;}
        let hdr=&*(l as *const NMHDR);
        if hdr.code!=NM_CUSTOMDRAW {return None;}
        let draw=&*(l as *const NMCUSTOMDRAW);
        let dc=draw.hdc;let hwnd=hdr.hwndFrom;
        let mut class=[0u16;64];let n=GetClassNameW(hwnd,class.as_mut_ptr(),64);
        let class=String::from_utf16_lossy(&class[..n as usize]);
        if class=="msctls_trackbar32" {
            if draw.dwDrawStage==CDDS_PREPAINT {fill(dc,&draw.rc,BACK);return Some(CDRF_NOTIFYITEMDRAW as isize);}
            if draw.dwDrawStage==CDDS_ITEMPREPAINT {
                let enabled=IsWindowEnabled(hwnd)!=0;
                fill(dc,&draw.rc,if draw.dwItemSpec==TBCD_THUMB as usize {if enabled {ACCENT}else{0x00555555}}else{0x00505050});
                return Some(CDRF_SKIPDEFAULT as isize);
            }
            return None;
        }
        if class!="Button" || draw.dwDrawStage!=CDDS_PREPAINT {return None;}
        let style=GetWindowLongPtrW(hwnd,GWL_STYLE) as u32;
        let ty=style & BS_TYPEMASK as u32;
        let check=ty==BS_CHECKBOX as u32 || ty==BS_RADIOBUTTON as u32;
        let checked=SendMessageW(hwnd,BM_GETCHECK,0,0)==BST_CHECKED as isize;
        let enabled=IsWindowEnabled(hwnd)!=0;
        let push=!check || style&BS_PUSHLIKE as u32!=0;
        let mut rc=draw.rc;
        fill(dc,&rc,BACK);
        let fg=if enabled {TEXT}else{MUTED};
        if push {
            let bg=if checked || draw.uItemState&CDIS_SELECTED!=0 {0x00594432}else if draw.uItemState&CDIS_HOT!=0 {0x00444444}else{FIELD};
            fill(dc,&rc,bg);border(dc,&rc,if checked {ACCENT}else{0x00606060});
        } else {
            let size=(13.*self.panel_scale).round() as i32;
            let top=rc.top+(rc.bottom-rc.top-size)/2;
            let mark=RECT{left:rc.left+2,top,right:rc.left+2+size,bottom:top+size};
            let pen=CreatePen(PS_SOLID,1,if checked {ACCENT}else{MUTED});
            let oldpen=SelectObject(dc,pen);SetDCBrushColor(dc,if checked {ACCENT}else{FIELD});let oldbrush=SelectObject(dc,GetStockObject(DC_BRUSH));
            if ty==BS_RADIOBUTTON as u32 {Ellipse(dc,mark.left,mark.top,mark.right,mark.bottom);} else {RoundRect(dc,mark.left,mark.top,mark.right,mark.bottom,4,4);}
            SelectObject(dc,oldbrush);SelectObject(dc,oldpen);DeleteObject(pen);
            if checked {
                let pad=(size as f32*0.30) as i32;
                let inner=RECT{left:mark.left+pad,top:mark.top+pad,right:mark.right-pad,bottom:mark.bottom-pad};
                if ty==BS_RADIOBUTTON as u32 {fill(dc,&inner,BACK);} else {
                    let pen=CreatePen(PS_SOLID,(2.*self.panel_scale).round() as i32,BACK);let old=SelectObject(dc,pen);
                    MoveToEx(dc,mark.left+size/5,top+size/2,null_mut());LineTo(dc,mark.left+size*2/5,top+size*3/4);LineTo(dc,mark.left+size*4/5,top+size/4);SelectObject(dc,old);DeleteObject(pen);
                }
            }
            rc.left=mark.right+(5.*self.panel_scale) as i32;
        }
        if hdr.idFrom==169 && self.github_images!=0 {
            let size=(18.*self.panel_scale).round() as i32;
            ImageList_Draw(self.github_images,0,dc,rc.left+(8.*self.panel_scale) as i32,rc.top+(rc.bottom-rc.top-size)/2,ILD_TRANSPARENT);
            rc.left+=size+(12.*self.panel_scale) as i32;
        }
        let mut text=[0u16;256];let len=GetWindowTextW(hwnd,text.as_mut_ptr(),256);
        let oldfont=SelectObject(dc,self.font);SetBkMode(dc,TRANSPARENT as i32);SetTextColor(dc,fg);
        DrawTextW(dc,text.as_ptr(),len,&mut rc,DT_SINGLELINE|DT_VCENTER|if push {DT_CENTER}else{DT_LEFT});SelectObject(dc,oldfont);
        if draw.uItemState&CDIS_FOCUS!=0 {let mut focus=draw.rc;InflateRect(&mut focus,-3,-3);DrawFocusRect(dc,&focus);}
        Some(CDRF_SKIPDEFAULT as isize)
    }
}
