use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use windows_sys::Win32::{Foundation::*,System::Threading::*,UI::WindowsAndMessaging::PostMessageW};
pub const MESSAGE:u32=0x8000+20;
static PENDING:AtomicBool=AtomicBool::new(false);
pub fn acknowledge(){PENDING.store(false,Ordering::Release);}
pub struct MenuClock {stop:Arc<AtomicBool>,thread:Option<std::thread::JoinHandle<()>>}
impl MenuClock {
    pub fn start(hwnd:HWND,fps:u32)->Self {
        let stop=Arc::new(AtomicBool::new(false));let flag=stop.clone();let window=hwnd as usize;
        acknowledge();
        let thread=std::thread::spawn(move||unsafe {
            let timer=CreateWaitableTimerExW(std::ptr::null(),std::ptr::null(),CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,TIMER_ALL_ACCESS);
            let period=std::time::Duration::from_secs_f64(1./fps.max(30) as f64);
            while !flag.load(Ordering::Acquire) {
                let due=-((period.as_nanos()/100) as i64);
                if !timer.is_null() && SetWaitableTimer(timer,&due,0,None,std::ptr::null(),0)!=0 {
                    WaitForSingleObject(timer,100);
                } else {std::thread::sleep(period);}
                if flag.load(Ordering::Acquire){break;}
                if !PENDING.swap(true,Ordering::AcqRel) && PostMessageW(window as HWND,MESSAGE,0,0)==0 {acknowledge();}
            }
            if !timer.is_null(){CancelWaitableTimer(timer);CloseHandle(timer);}
        });
        Self{stop,thread:Some(thread)}
    }
}
impl Drop for MenuClock {
    fn drop(&mut self){self.stop.store(true,Ordering::Release);if let Some(t)=self.thread.take(){let _=t.join();}acknowledge();}
}
