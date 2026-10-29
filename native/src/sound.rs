// PAT喵！点击音效：两段喵喵声随机播放，音量由面板「音效大小」滑块控制。
//
// 不引入第三方音频 crate：直接调用系统 winmm 的 PlaySoundW，从内存播放内嵌的
// WAV（PCM 16-bit / 22050 Hz / mono）。
//
// 快速连点由一条常驻音频线程处理：每次点击发一个请求，线程只保留最新请求，
// 先准备好新 WAV，再立刻 PlaySound(NULL) 切断上一段、马上起播下一段（不留空白）；
// 距上一次起播不足 15ms 的请求视为“无效帧”直接丢弃，避免手抖/双击抖动重复触发。
#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(psz_sound: *const u16, hmod: *mut core::ffi::c_void, flags: u32) -> i32;
}

use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

const SND_ASYNC: u32 = 0x0001;
const SND_NODEFAULT: u32 = 0x0002;
const SND_MEMORY: u32 = 0x0004;
/// 超快速点击的“无效帧”：距上次起播不足 15ms 的音效请求直接丢弃。
const DEBOUNCE: Duration = Duration::from_millis(15);

static MEOW: &[u8] = include_bytes!("../assets/pat-meow.wav");
static MEOW_HIGH: &[u8] = include_bytes!("../assets/pat-meow-high.wav");
static TX: OnceLock<mpsc::Sender<Request>> = OnceLock::new();

struct Request {
    index: usize,
    volume: f32,
}

/// index 0 = 喵喵，1 = 喵喵_游戏内高音调；volume 0..1。
pub fn play(index: usize, volume: f32) {
    let volume = volume.clamp(0., 1.);
    if volume <= 0.001 {
        return;
    }
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("pat-meow".into())
            .spawn(move || audio_loop(rx));
        tx
    });
    let _ = tx.send(Request { index, volume });
}

fn audio_loop(rx: mpsc::Receiver<Request>) {
    let mut current: Vec<u8> = Vec::new();
    let mut last_start: Option<Instant> = None;
    while let Ok(mut request) = rx.recv() {
        // 超快速点击时只处理已排队的最新一条，不逐条排队/拖慢。
        while let Ok(next) = rx.try_recv() {
            request = next;
        }
        let now = Instant::now();
        if last_start.is_some_and(|start| now.duration_since(start) < DEBOUNCE) {
            continue;
        }
        let data = if request.index & 1 == 0 { MEOW } else { MEOW_HIGH };
        let mut wav = data.to_vec();
        scale_pcm16(&mut wav, request.volume);
        // 先备好下一段，再切断上一段，最后立刻起播；空白间隙只有一次系统调用。
        unsafe {
            PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
            current = wav;
            PlaySoundW(
                current.as_ptr() as *const u16,
                std::ptr::null_mut(),
                SND_ASYNC | SND_MEMORY | SND_NODEFAULT,
            );
        }
        last_start = Some(now);
    }
}

/// 找到 WAV 的 data 块，把 16-bit PCM 样本按 volume 缩放（原地）。
fn scale_pcm16(wav: &mut [u8], volume: f32) {
    let mut pos = 12usize;
    while pos + 8 <= wav.len() {
        let size =
            u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        if &wav[pos..pos + 4] == b"data" {
            let start = pos + 8;
            let end = (start + size).min(wav.len());
            for sample in wav[start..end].chunks_exact_mut(2) {
                let value = i16::from_le_bytes([sample[0], sample[1]]) as f32 * volume;
                let value = value.round().clamp(-32768., 32767.) as i16;
                sample.copy_from_slice(&value.to_le_bytes());
            }
            break;
        }
        pos += 8 + size + (size & 1);
    }
}

#[cfg(test)]
mod tests {
    use super::scale_pcm16;

    #[test]
    fn scales_pcm16_samples_by_volume() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&36u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&22050u32.to_le_bytes());
        wav.extend_from_slice(&44100u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&1000i16.to_le_bytes());
        wav.extend_from_slice(&(-2000i16).to_le_bytes());
        scale_pcm16(&mut wav, 0.5);
        assert_eq!(i16::from_le_bytes([wav[44], wav[45]]), 500);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), -1000);
    }
}
