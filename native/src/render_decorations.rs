use super::*;
fn materials()-> &'static Vec<(RgbaImage,Vec<[[u8;3];6]>)> {
    static MATERIALS:std::sync::OnceLock<Vec<(RgbaImage,Vec<[[u8;3];6]>)>>=std::sync::OnceLock::new();
    MATERIALS.get_or_init(|| {
        let mut palette=vec![[244,240,241,255],[246,164,184,255],[65,62,69,255],[244,164,186,255],[250,247,237,255],[224,42,55,255]];
        let heart=image::load_from_memory(include_bytes!("../assets/heart.png")).unwrap().to_rgba8();
        for p in heart.pixels() {if p[3]>0 && !palette.contains(&p.0) {palette.push(p.0);}}
        palette.into_iter().map(|p| {let image=RgbaImage::from_pixel(1,1,image::Rgba(p));let colors=colors_for(&image);(image,colors)}).collect()
    })
}
impl Frame {
    pub(super) fn draw_decorations(&mut self,s:&Settings,skin:&Skin,pose:&Pose) {
        if s.decoration=="none" {return;}
        let parts=parts_with_cape(s,skin,pose,None);
        let Some(head)=parts.iter().find(|p|!p.keyboard && !p.cape && p.inflate==0. && p.uv==Vec2::ZERO) else{return;};
        // Head's complete matrix includes gaze, custom pose, Q proportions and dance.
        let parent=head.transform*Mat4::from_scale(Vec3::splat(head.size.y/8.));
        if s.decoration=="axolotl" {
            let model=decoration_models::get(&s.decoration);
            // Uniform scaling preserves the original creature's proportions.
            // All posed geometry stays above the head's outer skin (Y=4.5).
            let scale=0.36;
            let elevation=4.55-model.bottom*scale;
            let attach=parent*Mat4::from_translation(Vec3::Y*elevation)*Mat4::from_scale(Vec3::splat(scale));
            for original in &model.parts {
                let mut part=original.clone();part.transform=attach*part.transform;
                self.draw_box(&part,Tex::of(&model.image,&model.colors));
            }
            return;
        }
        let mut cube=|center:Vec3,size:Vec3,color:[u8;4]| {
            let (image,colors)=materials().iter().find(|(image,_)|image.get_pixel(0,0).0==color).unwrap();
            let part=BoxPart{size,uv:Vec2::ZERO,uv_size:None,transform:parent*Mat4::from_translation(center),
                inflate:0.,mirror:false,keyboard:false,cape:false,faces:None,eyes:None,eyelid:1.,pupil:Vec2::ZERO};
            self.draw_box(&part,Tex::of(image,colors));
        };
        match s.decoration.as_str() {
            "lollipop"=>{
                // Disc centered at the head-right upper front corner. The entire
                // ornament is in front of the outer skin; the handle points OUT.
                let angle=0.55f32;
                let origin=Vec3::new(-3.6,4.5,4.85);
                let point=|x:f32,y:f32|origin+Vec3::new(angle.cos()*x-angle.sin()*y,angle.sin()*x+angle.cos()*y,0.);
                // Square pixels keep the diagonal stick and round disc Minecraft-like.
                for i in 0..14 {let t=1.7+i as f32*0.27;cube(origin+Vec3::new(-0.8*t,-0.6*t,0.),Vec3::new(0.32,0.32,0.28),[250,247,237,255]);}
                let mut spiral=Vec::new();
                for i in 0..180 {let t=i as f32/179.;let a=t*std::f32::consts::TAU*1.8;let radius=0.8+t*7.2;spiral.push(Vec2::new(a.cos(),a.sin())*radius);}
                for y in -8i32..=8 {for x in -8i32..=8 {
                    let p=Vec2::new(x as f32,y as f32);if p.length()>8.2 {continue;}
                    let red=spiral.iter().any(|q|p.distance_squared(*q)<0.85*0.85);
                    cube(point(x as f32*0.23,y as f32*0.23),Vec3::new(0.245,0.245,0.38),if red {[224,42,55,255]}else{[250,247,237,255]});
                }}
            }
            "rabbit"=>for sign in [-1.,1.] {
                cube(Vec3::new(sign*2.3,6.9,0.),Vec3::new(1.7,5.,1.),[244,240,241,255]);
                cube(Vec3::new(sign*2.3,6.9,0.53),Vec3::new(0.8,3.7,0.12),[246,164,184,255]);
            },
            "cat"=>for sign in [-1.,1.] {for row in 0..3 {
                let width=2.6-row as f32*0.7;
                cube(Vec3::new(sign*(2.5+row as f32*0.15),4.9+row as f32*0.7,0.),Vec3::new(width,0.75,1.2),[65,62,69,255]);
                if row<2 {cube(Vec3::new(sign*(2.5+row as f32*0.15),4.95+row as f32*0.7,0.63),Vec3::new(width-0.8,0.55,0.1),[244,164,186,255]);}
            }},
            "heart"=>{
                static HEART:std::sync::OnceLock<RgbaImage>=std::sync::OnceLock::new();
                let heart=HEART.get_or_init(||image::load_from_memory(include_bytes!("../assets/heart.png")).unwrap().to_rgba8());
                for y in 0..8 {for x in 0..8 {let p=heart.get_pixel(x,y);if p[3]>0 {
                    cube(Vec3::new((x as f32-3.5)*0.55,8.4-y as f32*0.55,0.),Vec3::new(0.55,0.55,0.35),p.0);
                }}}
            }
            _=>{}
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn axolotl_tail_visible_from_both_sides() {
        let model=decoration_models::get("axolotl");
        let tail=model.parts.iter().find(|p|p.size==Vec3::new(0.,5.,12.)).unwrap();
        for angle in [-std::f32::consts::FRAC_PI_2,std::f32::consts::FRAC_PI_2] {
            let mut part=tail.clone();
            part.transform=Mat4::from_translation(Vec3::new(0.,15.,0.))*Mat4::from_rotation_y(angle);
            let mut f=Frame::new(360,420);
            f.draw_box(&part,Tex::of(&model.image,&model.colors));
            let visible=f.rgba().chunks_exact(4).filter(|p|p[3]>0).count();
            assert!(visible>100,"tail disappeared at angle {angle}: {visible} pixels");
        }
    }
    #[test] fn wine_period_scale_and_rotation() {
        let mut s=Settings::default();s.wine=true;let mut p=Pose::new(&s);
        let cycle=60./130.;
        for (time,height) in [(0.,1.),(cycle*0.5,0.5),(cycle,1.)] {
            p.clock=time;let v=wine_transform(&s,&p).transform_vector3(Vec3::Y);
            assert!((v.length()-height).abs()<0.0001);
        }
        p.clock=cycle;
        assert!(wine_transform(&s,&p).transform_vector3(Vec3::Z).abs_diff_eq(Vec3::X,0.0001));
    }
    #[test] fn decorations_render_for_both_models_and_follow_head() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for chibi in [false,true] {for choice in ["rabbit","cat","heart","axolotl","lollipop"] {
            let mut s=Settings::default();s.chibi=chibi;s.decoration=choice.into();
            let mut p=Pose::new(&s);let mut a=Frame::new(450,525);a.draw(&s,&skin,&p);
            let first=a.pixels.clone();p.head_rotation=Quat::from_rotation_z(0.4);
            a.draw(&s,&skin,&p);assert_ne!(a.pixels,first);
            s.decoration="none".into();let mut b=Frame::new(450,525);b.draw(&s,&skin,&p);
            assert_ne!(a.pixels,b.pixels);
        }}
        if let Ok(path)=std::env::var("DECORATION_REVIEW_PATH") {
            let mut sheet=RgbaImage::new(1440,840);
            for (row,choice) in ["axolotl","lollipop"].into_iter().enumerate() {
                for (col,(chibi,yaw)) in [(false,0.),(false,65.),(true,0.),(true,-65.)].into_iter().enumerate() {
                    let mut s=Settings::default();s.chibi=chibi;s.decoration=choice.into();
                    s.model_rotation=[0.,yaw,0.];s.track_mouse=false;s.outer_layer=true;
                    let mut p=Pose::new(&s);
                    for i in 0..60 {p.update(&s,&skin,Vec2::ZERO,360.,420.,false,i as f32/60.,1./60.);}
                    let mut f=Frame::new(360,420);f.draw(&s,&skin,&p);
                    let tile=RgbaImage::from_raw(360,420,f.rgba()).unwrap();
                    image::imageops::overlay(&mut sheet,&tile,(col*360) as i64,(row*420) as i64);
                }
            }
            sheet.save(path).unwrap();
        }
    }
}
