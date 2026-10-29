use super::*;
use std::sync::OnceLock;

// Candy palette sampled from the user's cape: pink, peach, lemon, mint,
// turquoise, sky blue and lavender. One choice per physical key press.
pub(super) const CANDY_COLORS:[[u8;3];9]=[
    [248,188,199],[248,176,234],[248,201,173],[248,225,156],
    [220,252,231],[106,222,216],[123,225,249],[155,210,248],[187,185,248],
];
pub(super) struct Letter {
    pub pos: Option<Vec2>, pub label: String, pub side: usize,
    pub hand_origin: Option<Vec3>, pub start: f32, pub color:[u8;3],
}
pub(super) struct Note { pub start:f32, pub phase:f32 }
pub(super) fn key_label(vk:u32)->Option<String> {
    if matches!(vk,48..=57 | 65..=90) { return Some((vk as u8 as char).to_string()); }
    if (96..=105).contains(&vk) { return Some((vk-96).to_string()); }
    if (112..=135).contains(&vk) { return Some(format!("F{}",vk-111)); }
    Some(match vk {
        8=>"Back",9=>"Tab",12=>"Clear",13|0x1000=>"Enter",
        16|160|161=>"Shift",17|162|163=>"Ctrl",18|164|165=>"Alt",
        19=>"Pause",20=>"CapsLock",27=>"Esc",32=>"Space",
        33=>"PageUp",34=>"PageDown",35=>"End",36=>"Home",
        37=>"Left",38=>"Up",39=>"Right",40=>"Down",
        44=>"PrintScreen",45=>"Insert",46=>"Delete",91|92=>"Win",93=>"Menu",
        106=>"*",107=>"+",108=>",",109=>"-",110=>".",111=>"/",
        144=>"NumLock",145=>"ScrollLock",
        166=>"BrowserBack",167=>"BrowserForward",168=>"Refresh",169=>"Stop",
        170=>"Search",171=>"Favorites",172=>"BrowserHome",
        173=>"Mute",174=>"VolumeDown",175=>"VolumeUp",176=>"NextTrack",
        177=>"PrevTrack",178=>"MediaStop",179=>"PlayPause",
        180=>"Mail",181=>"Media",182=>"App1",183=>"App2",
        186=>";",187=>"=",188=>",",189=>"-",190=>".",191=>"/",192=>"`",
        219=>"[",220|226=>"\\",221=>"]",222=>"'",
        1..=6 | 0=>return None,
        _=>return Some(format!("Key{vk:X}")),
    }.into())
}
struct Apple { image:RgbaImage, colors:Vec<[[u8;3];6]> }
fn apple()-> &'static Apple {
    static CACHE:OnceLock<Apple>=OnceLock::new();
    CACHE.get_or_init(|| {
        let image=image::load_from_memory(include_bytes!("../assets/apple.png")).expect("bundled apple").to_rgba8();
        let colors=colors_for(&image);Apple {image,colors}
    })
}

// ==== 「可调戏」长按手部素材 ====
// 由 tools/build_hand_asset.py 从用户提供的参考视频里抠出：只有手掌+手指，
// 不含手腕/前臂，也不做任何延展；5 帧正好是视频里循环的那 5 张画。
// 容器格式（小端序）：
//   "MCPH" u16 ver=1 u16 count u16 cell_w u16 cell_h
//   f32 fps f32 grip_x f32 grip_y f32 grip_diameter
//   f32 squash[count]
//   u32 png_len[count] + PNG 数据
const TEASE_HAND_BIN: &[u8] = include_bytes!("../assets/tease-hand.bin");
struct TeaseHand {
    /// 解码后已经预乘 alpha 的帧，直接用于双线性采样与合成。
    frames: Vec<RgbaImage>,
    cell: (usize, usize),
    grip: Vec2,
    grip_diameter: f32,
    squash: Vec<f32>,
}
static TEASE_HAND: OnceLock<Option<TeaseHand>> = OnceLock::new();
fn tease_hand() -> Option<&'static TeaseHand> {
    TEASE_HAND.get_or_init(TeaseHand::load).as_ref()
}
/// 每张循环画的垂直压缩系数；素材缺失时退回 1.0（只影响压缩幅度，不会 panic）。
pub(super) fn hand_squash(index: usize) -> f32 {
    tease_hand()
        .and_then(|h| h.squash.get(index).copied())
        .unwrap_or(1.0)
}
fn premultiply(image: RgbaImage) -> RgbaImage {
    let mut out = image;
    for p in out.pixels_mut() {
        let a = p[3] as u32;
        p[0] = (p[0] as u32 * a / 255) as u8;
        p[1] = (p[1] as u32 * a / 255) as u8;
        p[2] = (p[2] as u32 * a / 255) as u8;
    }
    out
}
fn sample_bilinear(img: &RgbaImage, fx: f32, fy: f32) -> [f32; 4] {
    let w = img.width() as i32;
    let h = img.height() as i32;
    if w <= 0 || h <= 0 { return [0.; 4]; }
    let x = fx.clamp(0., (w - 1) as f32);
    let y = fy.clamp(0., (h - 1) as f32);
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let px = |xx: i32, yy: i32| img.get_pixel(xx as u32, yy as u32).0.map(|v| v as f32);
    let p00 = px(x0, y0);
    let p10 = px(x1, y0);
    let p01 = px(x0, y1);
    let p11 = px(x1, y1);
    let mut out = [0.; 4];
    for c in 0..4 {
        let top = p00[c] + (p10[c] - p00[c]) * tx;
        let bot = p01[c] + (p11[c] - p01[c]) * tx;
        out[c] = top + (bot - top) * ty;
    }
    out
}
impl TeaseHand {
    fn load() -> Option<Self> {
        let b = TEASE_HAND_BIN;
        let header = 28;
        if b.len() < header
            || b[0] != b'M'
            || b[1] != b'C'
            || b[2] != b'P'
            || b[3] != b'H'
        {
            return None;
        }
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as usize;
        let f32_at = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        if u16_at(4) != 1 { return None; }
        let count = u16_at(6);
        let cell_w = u16_at(8);
        let cell_h = u16_at(10);
        if count == 0 || count > 64 || cell_w == 0 || cell_h == 0 { return None; }
        let fps = f32_at(12);
        if !(fps.is_finite() && fps > 0.) { return None; }
        let grip = Vec2::new(f32_at(16), f32_at(20));
        let grip_diameter = f32_at(24);
        if !(grip_diameter.is_finite() && grip_diameter > 0.01) { return None; }
        let lens_at = header + count * 4;
        if b.len() < lens_at + count * 4 { return None; }
        let mut squash = Vec::with_capacity(count);
        for i in 0..count {
            let value = f32_at(header + i * 4);
            squash.push(if value.is_finite() { value } else { 1. });
        }
        let mut lens = Vec::with_capacity(count);
        for i in 0..count {
            let o = lens_at + i * 4;
            lens.push(u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize);
        }
        let mut offset = lens_at + count * 4;
        let mut frames = Vec::with_capacity(count);
        for len in lens {
            if offset + len > b.len() { return None; }
            let image = image::load_from_memory(&b[offset..offset + len]).ok()?.to_rgba8();
            if image.width() as usize != cell_w || image.height() as usize != cell_h {
                return None;
            }
            frames.push(premultiply(image));
            offset += len;
        }
        Some(Self { frames, cell: (cell_w, cell_h), grip, grip_diameter, squash })
    }
}

impl Frame {
    /// 把当前循环帧的手部素材画在头部：抓点对准头部中心，整只手随「可调戏」
    /// 的透明度/弹入缩放淡入淡出。只画手本身，不带手腕/前臂。
    pub(super) fn draw_tease_hand(&mut self, s: &Settings, skin: &Skin, pose: &Pose) {
        if pose.hand_alpha <= 0.004 { return; }
        let Some(hand) = tease_hand() else { return; };
        if hand.frames.is_empty() || hand.grip_diameter <= 0.01 { return; }
        let parts = parts_with_cape(s, skin, pose, None);
        let head_index = if s.chibi || !s.outer_layer { 0 } else { 1 };
        if parts.len() <= head_index { return; }
        let head = &parts[head_index];
        let center = head.transform.transform_point3(Vec3::ZERO);
        let left = head.transform.transform_point3(Vec3::new(-head.size.x * 0.5, 0., 0.));
        let right = head.transform.transform_point3(Vec3::new(head.size.x * 0.5, 0., 0.));
        let w = self.width as f32;
        let h = self.height as f32;
        let head_px = (project(left, w, h).x - project(right, w, h).x).abs();
        let base = project(center, w, h);
        let anchor = Vec2::new(base.x + w * 0.012, base.y + h * 0.008);
        let pop = pose.hand_pop.clamp(0.5, 1.2);
        // 抓点直径 ≈ 1.35 个头宽：手包住脑袋但不至于把整只小人盖住。
        let scale = (head_px * 1.35 / hand.grip_diameter) * pop;
        if !(scale.is_finite() && scale > 0.01) { return; }
        let (cell_w, cell_h) = hand.cell;
        let dest_w = (cell_w as f32 * scale).round().max(1.) as i32;
        let dest_h = (cell_h as f32 * scale).round().max(1.) as i32;
        let drop = ((1. - pop) * h * 0.02).round() as i32;
        let left_i = (anchor.x - hand.grip.x * scale).round() as i32;
        let top_i = (anchor.y - hand.grip.y * scale).round() as i32 + drop;
        let index = pose.hand_frame.min(hand.frames.len() - 1);
        let img = &hand.frames[index];
        let alpha = pose.hand_alpha.clamp(0., 1.);
        for dy in 0..dest_h {
            let py = top_i + dy;
            if py < 0 || py >= self.height as i32 { continue; }
            let sy = (dy as f32 + 0.5) * cell_h as f32 / dest_h as f32 - 0.5;
            for dx in 0..dest_w {
                let px = left_i + dx;
                if px < 0 || px >= self.width as i32 { continue; }
                let sx = (dx as f32 + 0.5) * cell_w as f32 / dest_w as f32 - 0.5;
                let src = sample_bilinear(img, sx, sy);
                let a = (src[3] / 255. * alpha).clamp(0., 1.);
                if a < 0.004 { continue; }
                let i = (py as usize * self.width + px as usize) * 4;
                let inv = 1. - a;
                for c in 0..3 {
                    // Frame 像素是 BGRA（draw_box/overlay 都写 color[2-c]，Surface 直接
                    // 交给 UpdateLayeredWindow），所以这里也要把贴图的 R/G/B 反过来写，
                    // 否则肤色手会显示成蓝色。
                    let value = src[2 - c] * alpha + self.pixels[i + c] as f32 * inv;
                    self.pixels[i + c] = value.round().clamp(0., 255.) as u8;
                }
                let value = src[3] * alpha + self.pixels[i + 3] as f32 * inv;
                self.pixels[i + 3] = value.round().clamp(0., 255.) as u8;
                self.bounds[0] = self.bounds[0].min(px);
                self.bounds[1] = self.bounds[1].min(py);
                self.bounds[2] = self.bounds[2].max(px + 1);
                self.bounds[3] = self.bounds[3].max(py + 1);
            }
        }
    }
}

impl Frame {
    pub(super) fn draw_effects(&mut self,s:&Settings,skin:&Skin,pose:&Pose) {
        if let Some(height)=pose.question_height() {
            let boxes=parts_with_cape(s,skin,pose,None);
            let head=&boxes[if s.chibi {0} else {1}];
            let top=head.transform.transform_point3(Vec3::new(0.,head.size.y/2.+0.5,0.));
            let origin=project(top,self.width as f32,self.height as f32);
            let scale=self.width as f32/44.;
            let size=(4.2*scale).max(12.);
            let unit=(size/7.).max(1.);
            // Original seven-row pixel question mark; solid pink and shadow.
            let rows=[0b01110u8,0b11011,0b00011,0b00110,0b00100,0,0b00100];
            for shadow in [true,false] {
                let offset=if shadow {unit*0.6} else {0.};
                for (y,row) in rows.iter().enumerate() {for x in 0..5 {
                    if row&(1<<(4-x))==0 {continue;}
                    let left=(origin.x-2.5*unit+x as f32*unit+offset).round() as i32;
                    let top=(origin.y-2.*scale-4.*scale*height-size/2.+y as f32*unit+offset).round() as i32;
                    for yy in 0..unit.ceil() as i32 {for xx in 0..unit.ceil() as i32 {
                        self.overlay(left+xx,top+yy,if shadow {[98,46,77]} else {[255,158,199]},1.);
                    }}
                }}
            }
        }
        if !pose.notes.is_empty() {
            static NOTE:OnceLock<RgbaImage>=OnceLock::new();
            let tex=NOTE.get_or_init(||image::load_from_memory(include_bytes!("../assets/note.png")).expect("note texture").to_rgba8());
            let parts=parts_with_cape(s,skin,pose,None);
            let head=&parts[if s.chibi {0} else {1}];
            let top=head.transform.transform_point3(Vec3::new(0.,head.size.y/2.+0.5,0.));
            let origin=project(top,self.width as f32,self.height as f32);
            let scale=self.width as f32/44.;
            for note in &pose.notes {
                let t=((pose.clock-note.start)/0.5).clamp(0.,1.);
                let x=origin.x+note.phase.sin()*1.6*scale;
                let y=origin.y-2.*scale-t*4.*scale;
                let size=(4.2*scale).max(12.) as i32;
                let color=[0.,2.094395,4.18879].map(|offset|((note.phase+offset).sin()*0.65+0.35).clamp(0.,1.));
                for yy in 0..size {for xx in 0..size {
                    let p=tex.get_pixel(xx as u32*tex.width()/size as u32,yy as u32*tex.height()/size as u32);
                    self.overlay(x as i32+xx-size/2,y as i32+yy-size/2,[ (p[0] as f32*color[0]) as u8,(p[1] as f32*color[1]) as u8,(p[2] as f32*color[2]) as u8],p[3] as f32/255.*(1.-t*t));
                }}
            }
        }
        if let Some(start)=pose.feed_start {
            let tick=(pose.clock-start)*20.;
            let parts=parts_with_cape(s,skin,pose,None);
            // The fourth opaque box is the right arm in both model layouts.
            if let Some(arm)=parts.iter().filter(|p|p.inflate==0.).nth(3) {
                if tick<31. {
                    let hand=arm.transform.transform_point3(if s.chibi {Vec3::new(-0.3,-1.8,0.4)} else {Vec3::new(if s.arm_model=="slim" {-0.5} else {-1.},-4.5,1.)});
                    let size=if s.chibi {0.34} else {0.55};
                    let offset=model_transform(s).transform_vector3(Vec3::new(0.,if s.chibi {2.2} else {4.1},-0.2));
                    // The chibi head overhangs its very short arms: bring the item
                    // to its mouth plane so the full-sized face cannot occlude it.
                    let center=if s.chibi {
                        let head=&parts[0];
                        let mouth=head.transform.transform_point3(Vec3::new(-1.6,-2.0,head.size.z/2.+1.0));
                        (hand+offset).lerp(mouth,smooth(tick/4.))
                    } else {hand+offset};
                    let tr=Mat4::from_translation(center)*Mat4::from_rotation_y(-0.2)*Mat4::from_rotation_z(-0.12);
                    self.apple_patch(tr,size,None);
                }
                if (8. ..32.).contains(&tick) {
                    let bite=if tick>=31. {31.} else {(tick/4.).floor()*4.};
                    let age=(tick-bite)/20.;
                    let count=if bite==31. {16} else {5};
                    let head=&parts[if s.chibi {0} else {1}];
                    let mouth=head.transform.transform_point3(Vec3::new(0.,-2.,4.2));
                    for i in 0..count {
                        let theta=i as f32*2.39996+bite*0.7;
                        let pos=mouth+Vec3::new(theta.cos()*age*9.,age*(4.+(i%3) as f32)-22.*age*age,age*(4.+(i%4) as f32));
                        let tr=Mat4::from_translation(pos)*Mat4::from_quat(rotation(age*4.+i as f32,theta,age*6.));
                        self.apple_patch(tr,0.18,Some((4+(i%3)*3,5+((i/3)%3)*3)));
                    }
                }
            }
        }
        if s.typing_letters && s.is_sitting() {
            for letter in &pose.letters {
                let t=((pose.clock-letter.start)/0.8).clamp(0.,1.);
                let rise=if s.chibi {3.7*1.5*s.chibi_body_scale/100.*0.9} else {10.8};
                let local = if let Some(origin) = letter.hand_origin { origin } else if let Some(pos) = letter.pos { if s.chibi {
                    let q = s.chibi_body_scale / 100.;
                    Vec3::new(pos.x * 0.4 * q, 3. + 0.45 * q, (2.8 + pos.y * 0.18) * q) * 1.5
                } else { Vec3::new(pos.x, 10.3, 9.4 + pos.y) } } else { continue };
                let world=model_transform(s).transform_point3(local);
                let mut screen=project(world,self.width as f32,self.height as f32);
                screen.y-=rise*t*self.width as f32/44.;
                self.label(&letter.label,screen,1.-t*t,letter.color);
            }
        }
    }
    fn apple_patch(&mut self,tr:Mat4,size:f32,patch:Option<(u32,u32)>) {
        let tex=apple();
        for (x,y,pixel) in tex.image.enumerate_pixels() {
            if pixel[3]<16 {continue;}
            let (ox,oy)=if let Some((px,py))=patch {
                if x<px || x>=px+3 || y<py || y>=py+3 {continue;}
                ((px+1) as f32,(py+1) as f32)
            } else {(7.5,7.5)};
            // Face UVs are expressed in 16-unit space; rasterizer converts to 64 units.
            let uv=FaceUv {u0:x as f32+0.1,v0:y as f32+0.1,u1:x as f32+0.9,v1:y as f32+0.9,rotation:0};
            let part=BoxPart {size:Vec3::splat(size),uv:Vec2::ZERO,uv_size:None,transform:tr*Mat4::from_translation(Vec3::new((x as f32-ox)*size,(oy-y as f32)*size,0.)),inflate:0.,mirror:false,keyboard:false,cape:false,faces:Some([uv;6]),eyes:None,eyelid:1.,pupil:Vec2::ZERO};
            self.draw_box(&part,Tex::of(&tex.image,&tex.colors));
        }
    }
    fn overlay(&mut self,x:i32,y:i32,color:[u8;3],alpha:f32) {
        if x<0 || y<0 || x>=self.width as i32 || y>=self.height as i32 {return;}
        let a=(alpha.clamp(0.,1.)*255.).round() as u32;if a==0{return;}
        let i=(y as usize*self.width+x as usize)*4;
        for c in 0..3 {self.pixels[i+c]=((color[2-c] as u32*a+self.pixels[i+c] as u32*(255-a)+127)/255) as u8;}
        self.pixels[i+3]=(a+(self.pixels[i+3] as u32*(255-a)+127)/255) as u8;
        self.bounds[0]=self.bounds[0].min(x);self.bounds[1]=self.bounds[1].min(y);
        self.bounds[2]=self.bounds[2].max(x+1);self.bounds[3]=self.bounds[3].max(y+1);
    }
    fn label(&mut self,text:&str,pos:Vec3,alpha:f32,color:[u8;3]) {
        let scale=(self.width as f32/360.).clamp(0.65,3.);
        let widths:Vec<f32>=text.bytes().map(|ch| {
            let glyph=&glyphs()[(ch.saturating_sub(32)).min(94) as usize];
            let left=(0..24).find(|&x|(0..24).any(|y|glyph[y*24+x]>0)).unwrap_or(8);
            let right=(0..24).rev().find(|&x|(0..24).any(|y|glyph[y*24+x]>0)).unwrap_or(15);
            (right-left+3) as f32*scale
        }).collect();
        let total: f32=widths.iter().sum();
        let mut x=pos.x-total/2.;
        for (ch,width) in text.bytes().zip(widths) {
            self.letter(ch,Vec3::new(x+width/2.,pos.y,pos.z),alpha,color);
            x+=width;
        }
    }
    fn letter(&mut self,ch:u8,pos:Vec3,alpha:f32,color:[u8;3]) {
        let glyph=&glyphs()[(ch.saturating_sub(32)).min(94) as usize];
        let scale=(self.width as f32/360.).clamp(0.65,3.);
        let size=(24.*scale).ceil() as i32;
        for pass in 0..2 {for y in 0..size {for x in 0..size {
            let coverage=glyph[((y as f32/scale) as usize).min(23)*24+((x as f32/scale) as usize).min(23)] as f32/255.;
            let offset=if pass==0 {scale.ceil() as i32} else {0};
            self.overlay(pos.x as i32-size/2+x+offset,pos.y as i32-size/2+y+offset,if pass==0 {[70,45,70]} else {color},alpha*coverage*if pass==0 {0.45} else {1.});
        }}}
    }
}
fn glyphs()-> &'static Vec<[u8;576]> {
    static GLYPHS:OnceLock<Vec<[u8;576]>>=OnceLock::new();
    GLYPHS.get_or_init(|| {
        let mut result=vec![[0;576];95];
        #[cfg(windows)] unsafe {
            use windows_sys::Win32::Graphics::Gdi::*;
            let dc=CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null(){return result;}
            let mut info:BITMAPINFO=std::mem::zeroed();info.bmiHeader.biSize=std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth=24;info.bmiHeader.biHeight=-24;info.bmiHeader.biPlanes=1;info.bmiHeader.biBitCount=32;
            let mut pixels=std::ptr::null_mut();
            let bmp=CreateDIBSection(dc,&info,DIB_RGB_COLORS,&mut pixels,std::ptr::null_mut(),0);
            if bmp.is_null(){DeleteDC(dc);return result;}
            let old=SelectObject(dc,bmp);
            let name:Vec<u16>="Microsoft YaHei UI".encode_utf16().chain(Some(0)).collect();
            let font=CreateFontW(-20,0,0,0,700,0,0,0,DEFAULT_CHARSET as u32,OUT_DEFAULT_PRECIS as u32,CLIP_DEFAULT_PRECIS as u32,ANTIALIASED_QUALITY as u32,DEFAULT_PITCH as u32,name.as_ptr());
            let oldfont=SelectObject(dc,if font.is_null(){GetStockObject(DEFAULT_GUI_FONT)}else{font});
            SetTextColor(dc,0xFFFFFF);SetBkColor(dc,0);SetBkMode(dc,OPAQUE as i32);
            for (i,glyph) in result.iter_mut().enumerate() {
                std::ptr::write_bytes(pixels,0,24*24*4);
                let ch=[i as u16+32];
                let mut extent:windows_sys::Win32::Foundation::SIZE=std::mem::zeroed();
                GetTextExtentPoint32W(dc,ch.as_ptr(),1,&mut extent);
                TextOutW(dc,(24-extent.cx)/2,(24-extent.cy)/2,ch.as_ptr(),1);GdiFlush();
                let bytes=std::slice::from_raw_parts(pixels as *const u8,24*24*4);
                for j in 0..576 {glyph[j]=bytes[j*4].max(bytes[j*4+1]).max(bytes[j*4+2]);}
            }
            SelectObject(dc,oldfont);if !font.is_null(){DeleteObject(font);}SelectObject(dc,old);DeleteObject(bmp);DeleteDC(dc);
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_include_function_modifier_and_numpad_keys() {
        for (vk,label) in [(8,"Back"),(115,"F4"),(20,"CapsLock"),(96,"0"),(105,"9"),
            (106,"*"),(107,"+"),(109,"-"),(110,"."),(111,"/"),(0x1000,"Enter"),
            (160,"Shift"),(163,"Ctrl"),(165,"Alt"),(144,"NumLock")] {
            assert_eq!(key_label(vk).as_deref(),Some(label));
        }
    }
    #[test]
    fn fallback_letters_use_selected_hand_and_keep_their_origin() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for chibi in [false,true] { for mode in ["precise","alternating"] {
            let mut s=Settings::default();s.posture="sitting".into();s.typing_letters=true;
            s.chibi=chibi;s.typing_mode=mode.into();
            let mut p=Pose::new(&s);
            p.key(96,true,0.,&s);p.key(97,true,0.,&s);
            p.update(&s,&skin,Vec2::ZERO,360.,420.,false,0.01,0.01);
            assert_eq!(p.letters.len(),2);
            assert_ne!(p.letters[0].side,p.letters[1].side);
            let boxes=parts(&s,&skin,&p);
            for letter in &p.letters {
                let arm=&boxes[2+letter.side];
                let expected=model_transform(&s).inverse().transform_point3(arm.transform.transform_point3(Vec3::new(0.,if chibi {-2.} else {-6.},0.)));
                assert!(letter.hand_origin.unwrap().abs_diff_eq(expected,1e-5));
            }
            let origin=p.letters[0].hand_origin;
            p.update(&s,&skin,Vec2::ZERO,360.,420.,false,0.2,0.1);
            assert_eq!(p.letters[0].hand_origin,origin);
            p.key(96,true,0.3,&s);assert_eq!(p.letters.len(),2);
            p.key(65,true,0.3,&s);
            assert_eq!(p.letters[2].pos.is_none(),mode=="alternating");
        }}
    }
    #[test]
    #[cfg(windows)]
    fn multi_character_labels_render_and_extend_past_one_glyph() {
        let mut f=Frame::new(360,420);
        f.label("CapsLock",Vec3::new(180.,100.,0.),1.,CANDY_COLORS[0]);
        assert!(f.bounds[2]-f.bounds[0]>60);
        assert!(f.pixels.chunks_exact(4).filter(|p|p[3]>0).count()>100);
    }
    #[test]
    fn feeding_renders_and_recovers_across_models_and_postures() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for chibi in [false,true] {for posture in ["standing","sitting"] {
            let mut s=Settings::default();s.chibi=chibi;s.posture=posture.into();s.da_dance=false;s.track_mouse=false;
            let mut p=Pose::new(&s);p.feed(0.);
            for step in 0..96 {p.update(&s,&skin,Vec2::ZERO,360.,420.,false,step as f32/120.,1./120.);}
            assert!(p.feed_blend>0.99);
            let mut f=Frame::new(360,420);f.draw(&s,&skin,&p);
            let red=f.pixels.chunks_exact(4).filter(|p|p[2]>120 && p[2]>p[1].saturating_add(45) && p[3]>128).count();
            assert!(red>5,"apple should be visible: {chibi} {posture}");
            if let Ok(dir)=std::env::var("PET_EFFECT_SHOTS") {
                image::save_buffer(std::path::Path::new(&dir).join(format!("feed-{chibi}-{posture}.png")),&f.rgba(),360,420,image::ColorType::Rgba8).unwrap();
            }
            p.click(0.8,false);assert!(p.swing_start.is_some());assert!(p.feed_start.is_none());
            for step in 96..300 {p.update(&s,&skin,Vec2::ZERO,360.,420.,false,step as f32/120.,1./120.);}
            assert!(p.feed_start.is_none());assert_eq!(p.feed_blend,0.);
        }}
    }
    #[test]
    fn letters_are_bounded_and_expire_without_repeat_flooding() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        let mut s=Settings::default();s.posture="sitting".into();s.typing_letters=true;
        let mut p=Pose::new(&s);p.key(65,true,0.,&s);p.key(65,true,0.1,&s);
        assert_eq!(p.letters.len(),1);
        p.update(&s,&skin,Vec2::ZERO,360.,420.,false,0.4,0.01);
        let mut f=Frame::new(360,420);f.draw(&s,&skin,&p);
        if let Ok(dir)=std::env::var("PET_EFFECT_SHOTS") {image::save_buffer(std::path::Path::new(&dir).join("typing-letter.png"),&f.rgba(),360,420,image::ColorType::Rgba8).unwrap();}
        p.update(&s,&skin,Vec2::ZERO,360.,420.,false,0.81,0.01);assert!(p.letters.is_empty());
        for n in 0..200 {p.key(65,false,1.,&s);p.key(65,true,1.+n as f32/1000.,&s);}
        assert_eq!(p.letters.len(),64);
    }
    #[test]
    fn mouse_mapping_matches_all_five_buttons() {
        let mut s=Settings::default();
        for (mode,expected) in [("right",[false;5]),("left",[true;5]),("mapped",[true,false,false,true,true]),("reverse",[false,true,true,false,false])] {
            s.main_hand=mode.into();s.clean();
            assert_eq!((0..5).map(|i|s.swing_left(i)).collect::<Vec<_>>(),expected);
        }
    }
    #[test]
    fn music_notes_repeat_every_half_second_and_expire_after_stop() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        let mut s=Settings::default();s.music_notes=true;
        let mut p=Pose::new(&s);p.music_playing=true;
        for step in 0..300 {
            let now=step as f32/120.;
            p.update(&s,&skin,Vec2::ZERO,360.,420.,false,now,1./120.);
            assert_eq!(p.notes.len(),1,"continuous playback should show one note");
            assert!(now-p.notes[0].start<0.5);
        }
        p.music_playing=false;
        p.update(&s,&skin,Vec2::ZERO,360.,420.,false,3.1,1./120.);
        assert!(p.notes.is_empty());
    }
}
