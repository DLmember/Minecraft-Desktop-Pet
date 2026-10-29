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
    // The macOS input layer maps Mac key codes onto these virtual keys; label them
    // as printed on a Mac keyboard. The glyph atlas is ASCII only, so no ⌘/⌥ symbols.
    #[cfg(target_os = "macos")]
    if let Some(label)=mac_key_label(vk) { return Some(label.into()); }
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
#[cfg(any(target_os = "macos", test))]
fn mac_key_label(vk:u32)->Option<&'static str> {
    Some(match vk {
        91|92=>"Cmd",18|164|165=>"Option",17|162|163=>"Control",93=>"Fn",
        8=>"Delete",46=>"FwdDel",13|0x1000=>"Return",
        _=>return None,
    })
}
struct Apple { image:RgbaImage, colors:Vec<[[u8;3];6]> }
fn apple()-> &'static Apple {
    static CACHE:OnceLock<Apple>=OnceLock::new();
    CACHE.get_or_init(|| {
        let image=image::load_from_memory(include_bytes!("../assets/apple.png")).expect("bundled apple").to_rgba8();
        let colors=colors_for(&image);Apple {image,colors}
    })
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
            let part=BoxPart {size:Vec3::splat(size),uv:Vec2::ZERO,transform:tr*Mat4::from_translation(Vec3::new((x as f32-ox)*size,(oy-y as f32)*size,0.)),inflate:0.,mirror:false,keyboard:false,cape:false,faces:Some([uv;6]),eyes:None,eyelid:1.,pupil:Vec2::ZERO};
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
static GLYPHS:OnceLock<Vec<[u8;576]>>=OnceLock::new();
pub(crate) fn install_glyphs(bytes: &[u8]) -> bool {
    if bytes.len() != 95 * 576 { return false; }
    GLYPHS.set(bytes.chunks_exact(576).map(|c| c.try_into().unwrap()).collect()).is_ok()
}
fn glyphs()-> &'static Vec<[u8;576]> {
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
            #[cfg(target_os = "macos")]
            let label=mac_key_label(vk).unwrap_or(label);
            assert_eq!(key_label(vk).as_deref(),Some(label));
        }
    }
    #[test]
    fn mac_labels_use_mac_key_names() {
        for (vk,label) in [(91,"Cmd"),(92,"Cmd"),(164,"Option"),(165,"Option"),(162,"Control"),
            (93,"Fn"),(8,"Delete"),(46,"FwdDel"),(0x1000,"Return")] {
            assert_eq!(mac_key_label(vk),Some(label));
        }
        assert_eq!(mac_key_label(65),None);
        assert_eq!(mac_key_label(160),None);
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
