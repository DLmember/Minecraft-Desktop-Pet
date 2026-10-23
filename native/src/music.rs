use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
unsafe extern "C" {fn pet_music_poll()->bool;fn pet_music_init()->bool;fn pet_music_close();}
pub struct Monitor {
    enabled:Arc<AtomicBool>,playing:Arc<AtomicBool>,stop:Arc<AtomicBool>,started:bool,
}
impl Monitor {
    pub fn new()->Self { Self {enabled:Arc::new(AtomicBool::new(false)),playing:Arc::new(AtomicBool::new(false)),stop:Arc::new(AtomicBool::new(false)),started:false} }
    pub fn set_enabled(&mut self,enabled:bool) {
        self.enabled.store(enabled,Ordering::Relaxed);
        if !enabled {self.playing.store(false,Ordering::Relaxed);}
        if !enabled || self.started {return;}
        let (enabled,playing,stop)=(self.enabled.clone(),self.playing.clone(),self.stop.clone());
        self.started=std::thread::Builder::new().name("music-status".into()).stack_size(128*1024).spawn(move || {
            let mut initialized=false;
            let mut last_signal=std::time::Instant::now()-std::time::Duration::from_secs(10);
            while !stop.load(Ordering::Relaxed) {
                if enabled.load(Ordering::Relaxed) {
                    if !initialized {initialized=unsafe{pet_music_init()};}
                    if initialized && unsafe{pet_music_poll()} {last_signal=std::time::Instant::now();}
                    playing.store(last_signal.elapsed().as_secs_f32()<1.5,Ordering::Relaxed);
                } else {
                    playing.store(false,Ordering::Relaxed);
                    if initialized {unsafe{pet_music_close()};initialized=false;}
                    last_signal=std::time::Instant::now()-std::time::Duration::from_secs(10);
                }
                std::thread::sleep(std::time::Duration::from_millis(750));
            }
            if initialized {unsafe{pet_music_close()};}
        }).is_ok();
    }
    pub fn playing(&self)->bool {self.playing.load(Ordering::Relaxed)}
}
impl Drop for Monitor {fn drop(&mut self){self.stop.store(true,Ordering::Relaxed);}}
