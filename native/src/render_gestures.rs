use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture { Wave, Bounce, Shy, Clap, Stretch, Tilt, Sleepy }
impl Gesture {
    fn duration(self) -> f32 {
        match self { Self::Wave=>1.7, Self::Bounce=>1.4, Self::Shy=>2.2,
            Self::Clap=>1.8, Self::Stretch=>3., Self::Tilt=>2.4, Self::Sleepy=>2.25 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn advance(p:&mut Pose,s:&Settings,skin:&Skin,now:f32) {
        p.update(s,skin,Vec2::ZERO,360.,420.,false,now,1./120.);
    }
    #[test]
    fn cheek_cover_preserves_size_clears_head_and_returns_without_jump() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for chibi in [false,true] {for scale in [50.,100.,180.] {for slim in [false,true] {for seated in [false,true] {
            let mut s=Settings::default();s.chibi=chibi;s.chibi_body_scale=scale;
            s.arm_model=if slim {"slim"} else {"classic"}.into();s.track_mouse=false;s.outer_layer=true;
            s.posture=if seated {"sitting"} else {"standing"}.into();
            let mut p=Pose::new(&s);advance(&mut p,&s,&skin,0.);
            let rest=parts(&s,&skin,&p);
            p.trigger_gesture(Gesture::Shy,0.);advance(&mut p,&s,&skin,0.7);
            let raised=parts(&s,&skin,&p);
            // Opaque order is stable (normal body/head/arms, chibi head/body/arms).
            let head=&raised[if chibi {0}else{1}];
            let inverse=head.transform.inverse();
            let shell_half=if chibi {4.05}else{4.5};
            for i in [2,3] {
                let arm=&raised[i];let baseline=&rest[i];
                let a=arm.transform.to_scale_rotation_translation().0;
                let b=baseline.transform.to_scale_rotation_translation().0;
                assert!(a.abs_diff_eq(b,1e-4));assert_eq!(arm.size,baseline.size);
                let half=arm.size*0.5 + if chibi {Vec3::new(if slim {0.1}else{0.075},1./12.,0.075)}else{Vec3::splat(0.25)};
                for x in [-half.x,half.x] {for y in [-half.y,half.y] {for z in [-half.z,half.z] {
                    let local=inverse.transform_point3(arm.transform.transform_point3(Vec3::new(x,y,z)));
                    assert!(local.z>shell_half,"cheek clearance chibi={chibi} scale={scale}");
                }}}
            }
            p.key(65,true,0.7,&s);advance(&mut p,&s,&skin,0.7);
            let interrupted=parts(&s,&skin,&p);
            for i in [2,3] {assert!(raised[i].transform.abs_diff_eq(interrupted[i].transform,1e-4));}
            for i in 1..=30 {advance(&mut p,&s,&skin,0.7+i as f32/120.);}
            assert_eq!(p.cheek_cover,0.);assert!(p.gesture_return.is_none());
        }}}}
    }
    #[test]
    #[ignore = "local DLmember visual review"]
    fn cheek_cover_visual_review() {
        let dir=std::path::PathBuf::from(std::env::var("SHY_REVIEW_DIR").unwrap());
        let skin=Skin::load(&std::fs::read(dir.join("skin.png")).unwrap()).unwrap();
        let mut sheet=image::RgbaImage::from_pixel(1440,1680,image::Rgba([225,229,235,255]));
        for (row,(chibi,scale)) in [(false,100.),(true,50.),(true,100.),(true,180.)].into_iter().enumerate() {
            for col in 0..4 {
                let mut s=Settings::default();s.chibi=chibi;s.chibi_body_scale=scale;
                s.arm_model="slim".into();s.outer_layer=true;s.track_mouse=false;
                s.posture=if col==3 {"sitting"} else {"standing"}.into();
                let mut p=Pose::new(&s);p.trigger_gesture(Gesture::Shy,0.);
                let now=[0.10,0.30,1.10,1.10][col];
                advance(&mut p,&s,&skin,now);
                let mut f=Frame::new(360,420);f.draw(&s,&skin,&p);
                let tile=image::RgbaImage::from_raw(360,420,f.rgba()).unwrap();
                image::imageops::overlay(&mut sheet,&tile,(col*360) as i64,(row*420) as i64);
            }
        }
        sheet.save(dir.join("shy11-review.png")).unwrap();
    }
    #[test]
    fn revised_gestures_match_angles_timing_and_gaze_rules() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for chibi in [false,true] {for scale in [50.,100.,180.] {
            let mut s=Settings::default();s.chibi=chibi;s.chibi_body_scale=scale;s.track_mouse=true;
            let mut a=Pose::new(&s);let mut b=Pose::new(&s);
            a.trigger_gesture(Gesture::Shy,0.);b.trigger_gesture(Gesture::Shy,0.);
            a.update(&s,&skin,Vec2::new(-5000.,-5000.),360.,420.,false,0.7,0.01);
            b.update(&s,&skin,Vec2::new(5000.,5000.),360.,420.,false,0.7,0.01);
            assert!(a.head_rotation.angle_between(b.head_rotation)<0.001);
            a.gesture_return=None;a.gesture=Some(ActiveGesture{kind:Gesture::Stretch,start:0.,automatic:true});
            advance(&mut a,&s,&skin,1.);
            for side in 0..2 {let direction=a.arms[side]*-Vec3::Y;
                assert!(direction.z.abs()<0.03);assert!((direction.y-3f32.sqrt()/2.).abs()<0.03);
                assert!((direction.x.abs()-0.5).abs()<0.03);
            }
            a.gesture=Some(ActiveGesture{kind:Gesture::Sleepy,start:0.,automatic:true});
            advance(&mut a,&s,&skin,2.125);
            let jump=a.gesture_jump*if chibi {scale/100.*0.6} else {1.};
            assert!((jump-if chibi {7.2*1.5*0.3} else {8.*0.3}).abs()<0.0001);
            advance(&mut a,&s,&skin,2.25);assert_eq!(a.gesture_jump,0.);
        }}
        let mut p=Pose::new(&Settings::default());
        p.gesture=Some(ActiveGesture{kind:Gesture::Tilt,start:0.,automatic:true});
        for (t,height) in [(0.,0.),(0.8,0.875),(1.6,1.),(2.,0.875)] {
            p.clock=t;assert!((p.question_height().unwrap()-height).abs()<1e-5);
        }
        p.clock=2.4;assert!(p.question_height().is_none());
    }
    #[test]
    fn occasional_deadlines_wait_without_queue_and_only_choose_enabled_actions() {
        let mut s=Settings::default();s.occasional_tilt=true;s.posture="sitting".into();
        let mut p=Pose::new(&s);p.random_state=123;
        p.schedule_gesture(&s,0.);
        let due=p.next_gesture.unwrap();assert!((30. ..=60.).contains(&due));
        p.key(65,true,1.,&s);
        p.schedule_gesture(&s,due+120.);
        assert!(p.gesture.is_none());assert_eq!(p.next_gesture,Some(due));
        p.key(65,false,due+120.,&s);p.schedule_gesture(&s,due+121.);
        assert_eq!(p.gesture.unwrap().kind,Gesture::Tilt);
        assert!(p.gesture.unwrap().automatic);
        assert!((due+151. ..=due+181.).contains(&p.next_gesture.unwrap()));
        s.occasional_tilt=false;p.schedule_gesture(&s,due+122.);
        assert!(p.gesture.is_none());assert!(p.next_gesture.is_none());
    }
    #[test]
    fn occasional_actions_leave_da_dance_phase_and_joints_intact() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for chibi in [false,true] {for kind in [Gesture::Stretch,Gesture::Tilt,Gesture::Sleepy] {
            let mut s=Settings::default();s.da_dance=true;s.chibi=chibi;
            let mut a=Pose::new(&s);let mut b=Pose::new(&s);
            a.gesture=Some(ActiveGesture {kind,start:0.,automatic:true});
            for frame in 0..420 {
                let now=frame as f32/120.;advance(&mut a,&s,&skin,now);advance(&mut b,&s,&skin,now);
                assert_eq!(a.dance_timer,b.dance_timer);assert_eq!(a.body_roll,b.body_roll);
                assert_eq!(a.leg_slide,b.leg_slide);assert_eq!(a.body_bob,b.body_bob);
                assert!(a.arms.iter().all(|q|q.is_finite() && (q.length()-1.).abs()<1e-4));
            }
            assert!(a.gesture.is_none());
        }}
    }
    #[test]
    fn manual_actions_recover_custom_pose_and_held_keys_and_keyboard_stays_put() {
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for chibi in [false,true] {for posture in ["standing","sitting","custom"] {
            for kind in [Gesture::Wave,Gesture::Bounce,Gesture::Shy,Gesture::Clap] {
                let mut s=Settings::default();s.chibi=chibi;s.posture=posture.into();s.custom_base="sitting".into();s.tease_enabled=true;
                s.custom_sitting_pose.left_arm=[-15.,10.,20.];s.track_mouse=false;
                let mut p=Pose::new(&s);
                for i in 0..100 {advance(&mut p,&s,&skin,i as f32/120.);}
                p.key(65,true,1.,&s);
                for i in 0..100 {advance(&mut p,&s,&skin,1.+i as f32/120.);}
                let rest=p.arms;let held=p.held_codes();
                let board=parts(&s,&skin,&p).into_iter().find(|p|p.keyboard);
                p.trigger_gesture(kind,2.);
                for i in 0..500 {advance(&mut p,&s,&skin,2.+i as f32/120.);
                    if let Some(board)=&board {
                        let current=parts(&s,&skin,&p).into_iter().find(|p|p.keyboard).unwrap();
                        assert!(current.transform.abs_diff_eq(board.transform,1e-5));
                    }
                }
                assert_eq!(p.held_codes(),held);assert_eq!(p.gesture_jump,0.);
                for side in 0..2 {assert!(p.arms[side].angle_between(rest[side])<0.002);}
                p.trigger_gesture(kind,7.);advance(&mut p,&s,&skin,7.4);
                let captured=p.arms;p.key(81,true,7.4,&s);
                assert!(p.gesture.is_none());advance(&mut p,&s,&skin,7.4);
                for side in 0..2 {assert!(p.arms[side].angle_between(captured[side])<0.002);}
                p.trigger_gesture(kind,8.);p.click(8.1,false);assert!(p.gesture.is_none());assert!(p.swing_start.is_some());
                p.trigger_gesture(kind,9.);p.tease(9.1);p.trigger_gesture(Gesture::Clap,9.2);
                assert!(p.gesture.is_none());assert!(p.tease_start.is_some());
            }
        }}
    }
    #[test]
    fn gesture_visual_samples() {
        let Ok(dir)=std::env::var("PET_GESTURE_SHOTS") else {return;};
        std::fs::create_dir_all(&dir).unwrap();
        let skin=Skin::load(DEFAULT_SKIN).unwrap();
        for kind in [Gesture::Wave,Gesture::Bounce,Gesture::Shy,Gesture::Clap,Gesture::Stretch,Gesture::Tilt,Gesture::Sleepy] {
            let mut sheet=image::RgbaImage::from_pixel(1440,1680,image::Rgba([230,232,237,255]));
            for row in 0..4 {
                let mut s=Settings::default();s.chibi=row>=2;s.posture=if row%2==1 {"sitting"} else {"standing"}.into();s.track_mouse=false;
                let mut p=Pose::new(&s);
                for i in 0..120 {advance(&mut p,&s,&skin,i as f32/120.);}
                p.trigger_gesture(kind,1.);
                for col in 0..4 {
                    let now=1.+kind.duration()*(col as f32+0.5)/4.;advance(&mut p,&s,&skin,now);
                    let mut frame=Frame::new(360,420);frame.draw(&s,&skin,&p);
                    let tile=image::RgbaImage::from_raw(360,420,frame.rgba()).unwrap();
                    image::imageops::overlay(&mut sheet,&tile,col*360,row*420);
                }
            }
            sheet.save(std::path::Path::new(&dir).join(format!("{kind:?}.png"))).unwrap();
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct ActiveGesture { kind: Gesture, start: f32, pub automatic: bool }
pub(super) struct ReturnPose { start: f32, head: Quat, arms: [Quat;2], jump: f32, cheek_cover: f32 }

impl Pose {
    fn remember_gesture_pose(&mut self, now: f32) {
        self.gesture_return=Some(ReturnPose {start:now,head:self.head_rotation,arms:self.arms,jump:self.gesture_jump,cheek_cover:self.cheek_cover});
    }
    pub(super) fn cancel_gesture(&mut self, now: f32) {
        if self.gesture.take().is_some() { self.remember_gesture_pose(now); }
    }
    pub(super) fn gesture_input(&mut self, now: f32) {
        self.last_gesture_input=now;
        self.cancel_gesture(now);
        if self.feed_start.take().is_some() {
            self.remember_gesture_pose(now);
            self.feed_blend=0.;
        }
    }
    pub fn trigger_gesture(&mut self, kind: Gesture, now: f32) {
        if self.tease_start.is_some() {return;}
        self.remember_gesture_pose(now);
        self.feed_start=None; self.feed_blend=0.; self.reset_swing();
        self.gesture=Some(ActiveGesture {kind,start:now,automatic:false});
        self.last_gesture_input=now;
    }
    /// Deadline check also runs while rasterization sleeps. No background thread
    /// and no extra continuous redraw are needed for a 30–60 second timer.
    pub fn gesture_due(&self, s: &Settings, now: f32) -> bool {
        let mask=Self::gesture_mask(s);
        mask!=self.occasional_mask || (mask!=0 && self.next_gesture.is_some_and(|t| now>=t)
            && self.gesture_idle(now))
    }
    fn gesture_mask(s: &Settings) -> u8 {
        s.occasional_stretch as u8 | ((s.occasional_tilt as u8)<<1) | ((s.occasional_sleepy as u8)<<2)
    }
    fn gesture_idle(&self, now: f32) -> bool {
        self.gesture.is_none() && self.gesture_return.is_none() && self.feed_start.is_none()
            && self.swing_start.is_none() && self.tease_start.is_none()
            && self.held.is_empty() && now-self.last_gesture_input>=0.6
    }
    fn next_gesture_time(&mut self, now: f32) { self.next_gesture=Some(now+30.+self.random_choice(30001) as f32/1000.); }
    pub(super) fn schedule_gesture(&mut self, s: &Settings, now: f32) {
        let mask=Self::gesture_mask(s);
        if mask!=self.occasional_mask {
            self.occasional_mask=mask;
            if self.gesture.is_some_and(|g|g.automatic) {self.cancel_gesture(now);}
            if mask==0 {self.next_gesture=None;} else {self.next_gesture_time(now);}
        }
        if mask==0 || !self.next_gesture.is_some_and(|t| now>=t) || !self.gesture_idle(now) {return;}
        let choices=[Gesture::Stretch,Gesture::Tilt,Gesture::Sleepy];
        let enabled:Vec<_>=choices.into_iter().enumerate().filter(|(i,_)|mask&(1<<i)!=0).map(|(_,g)|g).collect();
        let kind=enabled[self.random_choice(enabled.len())];
        self.gesture=Some(ActiveGesture {kind,start:now,automatic:true});
        self.next_gesture_time(now);
    }
    pub(super) fn apply_gesture(&mut self, s: &Settings, now: f32) {
        self.gesture_jump=0.;
        self.gesture_shake=Vec3::ZERO;
        self.cheek_cover=0.;
        if let Some(g)=self.gesture {
            let t=(now-g.start).max(0.);
            let duration=g.kind.duration();
            if t>=duration {self.gesture=None;} else {
                let w=smooth(t/0.22)*(1.-smooth((t-(duration-0.35))/0.35));
                let phase=t*std::f32::consts::TAU;
                match g.kind {
                    Gesture::Wave=>{
                        let wave=if s.chibi {
                            Quat::from_rotation_arc(-Vec3::Y,Vec3::new(-0.72,0.75+(phase*2.2).sin()*0.20,0.22).normalize())
                        } else {rotation(-2.95,0.,-0.20+(phase*2.2).sin()*0.20)};
                        self.arms[1]=self.arms[1].slerp(wave,w);
                        self.head_rotation*=rotation(0.,0.,-0.10*w);
                    }
                    Gesture::Bounce=>{
                        let flight=((t-0.12)/0.48).clamp(0.,2.);
                        let hop=if flight>0. && flight<2. {(flight.fract()*std::f32::consts::PI).sin()} else {0.};
                        self.gesture_jump=hop*3.6*w;
                        for side in 0..2 {self.arms[side]*=rotation(-0.12*w,0.,if side==0 {0.22*w} else {-0.22*w});}
                    }
                    Gesture::Shy=>{
                        self.cheek_cover=w;
                        // Keep the face visible while bowing into the raised hands.
                        let tucked=rotation(0.18,0.,(phase*0.4).sin()*0.025);
                        self.head_rotation=self.head_rotation.slerp(tucked,w);
                    }
                    Gesture::Clap=>{
                        let half_width=if s.chibi {0.6} else if s.arm_model=="slim" {1.5} else {2.};
                        let gap=half_width+1.95*(phase*2.4).sin().abs();
                        for side in 0..2 {
                            let sign=if side==0 {1.} else {-1.};
                            let shoulder=if s.chibi {2.35} else if s.arm_model=="slim" {5.5} else {6.};
                            let dx: f32=sign*(gap-shoulder);
                            let dy: f32=if s.chibi {-0.9} else {-2.8};
                            let length: f32=if s.chibi {4.} else {10.};
                            let reach=Vec3::new(dx,dy,(length*length-dx*dx-dy*dy).max(0.1).sqrt());
                            self.arms[side]=self.arms[side].slerp(Quat::from_rotation_arc(-Vec3::Y,reach.normalize()),w);
                        }
                    }
                    Gesture::Stretch=>{
                        // Relative overlays preserve custom poses and the live DA
                        // torso/hip transforms instead of replacing their matrices.
                        for side in 0..2 {
                            let sign=if side==0 {1.} else {-1.};
                            // User axes: forward +X, left +Y, up +Z.
                            // Renderer axes: left +X, up +Y, forward +Z.
                            let direction=Vec3::new(sign*0.5,3f32.sqrt()/2.,0.);
                            let target=Quat::from_rotation_arc(-Vec3::Y,direction);
                            let tremor=(phase*11.).sin()*0.018*w;
                            self.arms[side]=self.arms[side].slerp(target,w)*rotation(tremor,0.,sign*tremor);
                        }
                        self.gesture_shake=Vec3::new((phase*11.).sin(),(phase*13.).sin()*0.5,0.)*0.035*w;
                        self.head_rotation*=rotation(-0.12*w,0.,0.);
                    }
                    Gesture::Tilt=>self.head_rotation*=rotation(0.,0.,0.32*w*(phase*0.25).sin()),
                    Gesture::Sleepy=>{
                        let nod=if t<2. {smooth(t/2.)*0.42} else {0.};
                        self.head_rotation*=rotation(nod,0.,0.);
                        if t>=2. {
                            let u=((t-2.)/0.25).clamp(0.,1.);
                            // Chibi head is unchanged by the body slider. Undo
                            // the render-time jump scaling to preserve 30% height.
                            let height=if s.chibi {7.2*1.5*0.3/(s.chibi_body_scale/100.*0.6)} else {8.*0.3};
                            self.gesture_jump=4.*u*(1.-u)*height;
                        }
                    }
                }
            }
        }
        if let Some(from)=&self.gesture_return {
            let blend=smooth((now-from.start)/0.16);
            self.head_rotation=from.head.slerp(self.head_rotation,blend);
            for side in 0..2 {self.arms[side]=from.arms[side].slerp(self.arms[side],blend);}
            self.gesture_jump=from.jump+(self.gesture_jump-from.jump)*blend;
            self.cheek_cover=from.cheek_cover+(self.cheek_cover-from.cheek_cover)*blend;
            if blend>=1. {self.gesture_return=None;}
        }
    }
    pub(super) fn question_height(&self)->Option<f32> {
        let g=self.gesture?;
        if g.kind!=Gesture::Tilt {return None;}
        let t=(self.clock-g.start)/g.kind.duration();
        if !(0. ..1.).contains(&t) {return None;}
        Some(if t<2./3. {1.-(1.-t*1.5).powi(3)} else {1.-((t-2./3.)*3.).powi(3)})
    }
}
