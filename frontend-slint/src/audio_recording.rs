//! Small platform-native microphone capture for the Slint composer.
//!
//! The recorder deliberately returns PCM WAV bytes and has no async runtime or
//! codec dependency.  Windows uses the Win7-compatible WinMM waveIn API;
//! Linux uses the ubiquitous ALSA `default` capture device.  Both paths run on
//! a worker thread and enforce the same bounded duration.

use std::sync::{atomic::AtomicBool, Arc};
#[cfg(any(windows, target_os = "linux"))]
use std::sync::atomic::Ordering;

pub const MAX_RECORDING_SECONDS: u64 = 120;

#[derive(Debug)]
pub struct RecordedAudio {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub content_type: String,
}

pub fn spawn(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<Result<RecordedAudio, String>> {
    std::thread::spawn(move || record(stop))
}

fn record(stop: Arc<AtomicBool>) -> Result<RecordedAudio, String> {
    #[cfg(windows)]
    let pcm = record_windows(stop)?;
    #[cfg(target_os = "linux")]
    let pcm = record_linux(stop)?;
    #[cfg(not(any(windows, target_os = "linux")))]
    return Err("当前平台暂不支持录音".to_string());

    if pcm.is_empty() {
        return Err("未检测到录音数据".to_string());
    }
    Ok(RecordedAudio {
        bytes: pcm_to_wav(&pcm, 16_000, 1, 16),
        filename: "wunder-voice.wav".to_string(),
        content_type: "audio/wav".to_string(),
    })
}

fn pcm_to_wav(pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_len = pcm.len().min(u32::MAX as usize) as u32;
    let riff_len = 36u32.saturating_add(data_len);
    let mut wav = Vec::with_capacity(44usize.saturating_add(data_len as usize));
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_len.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&pcm[..data_len as usize]);
    wav
}

#[cfg(windows)]
fn record_windows(stop: Arc<AtomicBool>) -> Result<Vec<u8>, String> {
    use std::ptr::null_mut;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        Media::{
            Audio::{
                waveInAddBuffer, waveInClose, waveInOpen, waveInPrepareHeader, waveInReset,
                waveInStart, waveInStop, waveInUnprepareHeader, CALLBACK_EVENT, WAVEFORMATEX,
                WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
            },
            MMSYSERR_NOERROR,
        },
        System::Threading::{CreateEventW, WaitForSingleObject},
    };

    const SAMPLE_RATE: u32 = 16_000;
    // A half-second buffer keeps stop latency low while avoiding callback code.
    const BUFFER_BYTES: usize = SAMPLE_RATE as usize;
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let event = unsafe { CreateEventW(null_mut(), 0, 0, std::ptr::null()) };
    if event == 0 {
        return Err("无法创建录音同步事件".to_string());
    }
    let mut wave: isize = 0;
    let open = unsafe {
        waveInOpen(
            &mut wave,
            WAVE_MAPPER,
            &format,
            event as usize,
            0,
            CALLBACK_EVENT,
        )
    };
    if open != MMSYSERR_NOERROR {
        unsafe { CloseHandle(event) };
        return Err(format!("无法打开麦克风（WinMM 错误 {open}）"));
    }
    let mut data = vec![0u8; BUFFER_BYTES];
    let mut header = WAVEHDR {
        lpData: data.as_mut_ptr(),
        dwBufferLength: BUFFER_BYTES as u32,
        dwBytesRecorded: 0,
        dwUser: 0,
        dwFlags: 0,
        dwLoops: 0,
        lpNext: null_mut(),
        reserved: 0,
    };
    let header_size = std::mem::size_of::<WAVEHDR>() as u32;
    let cleanup = |wave: isize, event: isize, header: &mut WAVEHDR| unsafe {
        let _ = waveInReset(wave);
        let _ = waveInUnprepareHeader(wave, header, header_size);
        let _ = waveInClose(wave);
        let _ = CloseHandle(event);
    };
    for (name, result) in [
        ("准备录音缓冲区", unsafe {
            waveInPrepareHeader(wave, &mut header, header_size)
        }),
        ("提交录音缓冲区", unsafe {
            waveInAddBuffer(wave, &mut header, header_size)
        }),
        ("启动麦克风", unsafe { waveInStart(wave) }),
    ] {
        if result != MMSYSERR_NOERROR {
            cleanup(wave, event, &mut header);
            return Err(format!("{name}失败（WinMM 错误 {result}）"));
        }
    }

    let started = Instant::now();
    let mut pcm = Vec::new();
    let max_bytes = SAMPLE_RATE as usize * 2 * MAX_RECORDING_SECONDS as usize;
    loop {
        if stop.load(Ordering::Acquire)
            || started.elapsed() >= Duration::from_secs(MAX_RECORDING_SECONDS)
        {
            break;
        }
        let signaled = unsafe { WaitForSingleObject(event, 100) } == WAIT_OBJECT_0;
        if !signaled || header.dwFlags & WHDR_DONE == 0 {
            continue;
        }
        let recorded = (header.dwBytesRecorded as usize).min(data.len());
        if recorded > 0 {
            pcm.extend_from_slice(&data[..recorded]);
        }
        if pcm.len() >= max_bytes {
            break;
        }
        header.dwBytesRecorded = 0;
        header.dwFlags = 0;
        let result = unsafe { waveInAddBuffer(wave, &mut header, header_size) };
        if result != MMSYSERR_NOERROR {
            cleanup(wave, event, &mut header);
            return Err(format!("重新提交录音缓冲区失败（WinMM 错误 {result}）"));
        }
    }
    unsafe {
        let _ = waveInStop(wave);
        let _ = waveInReset(wave);
    }
    // Reset marks the active buffer done; retain the final partial block.
    if header.dwFlags & WHDR_DONE != 0 {
        let recorded = (header.dwBytesRecorded as usize).min(data.len());
        if recorded > 0 {
            pcm.extend_from_slice(&data[..recorded]);
        }
    }
    pcm.truncate(max_bytes);
    cleanup(wave, event, &mut header);
    Ok(pcm)
}

#[cfg(target_os = "linux")]
fn record_linux(stop: Arc<AtomicBool>) -> Result<Vec<u8>, String> {
    use std::ffi::CString;
    use std::os::raw::{c_int, c_uint, c_void};
    use std::time::{Duration, Instant};

    #[repr(C)]
    struct SndPcm(std::ffi::c_void);
    type SndPcmUFrames = usize;
    type SndPcmSFrames = isize;
    const STREAM_CAPTURE: c_int = 1;
    const ACCESS_INTERLEAVED: c_int = 3;
    const FORMAT_S16_LE: c_int = 2;
    #[link(name = "asound")]
    unsafe extern "C" {
        fn snd_pcm_open(
            handle: *mut *mut SndPcm,
            name: *const i8,
            stream: c_int,
            mode: c_int,
        ) -> c_int;
        fn snd_pcm_set_params(
            handle: *mut SndPcm,
            format: c_int,
            access: c_int,
            channels: c_uint,
            rate: c_uint,
            soft_resample: c_int,
            latency: c_uint,
        ) -> c_int;
        fn snd_pcm_readi(
            handle: *mut SndPcm,
            buffer: *mut c_void,
            size: SndPcmUFrames,
        ) -> SndPcmSFrames;
        fn snd_pcm_recover(handle: *mut SndPcm, error: c_int, silent: c_int) -> c_int;
        fn snd_pcm_close(handle: *mut SndPcm) -> c_int;
    }
    const RATE: usize = 16_000;
    let name = CString::new("default").map_err(|_| "无效的 ALSA 设备名称".to_string())?;
    let mut handle = std::ptr::null_mut();
    let result = unsafe { snd_pcm_open(&mut handle, name.as_ptr(), STREAM_CAPTURE, 0) };
    if result < 0 || handle.is_null() {
        return Err(format!("无法打开 ALSA 麦克风（错误 {result}）"));
    }
    let configured = unsafe {
        snd_pcm_set_params(
            handle,
            FORMAT_S16_LE,
            ACCESS_INTERLEAVED,
            1,
            RATE as c_uint,
            1,
            100_000,
        )
    };
    if configured < 0 {
        unsafe { snd_pcm_close(handle) };
        return Err(format!(
            "ALSA 不支持 16 kHz 单声道录音（错误 {configured}）"
        ));
    }
    let mut pcm = Vec::new();
    let mut frames = vec![0i16; 1600];
    let max_bytes = RATE * 2 * MAX_RECORDING_SECONDS as usize;
    let started = Instant::now();
    while !stop.load(Ordering::Acquire)
        && started.elapsed() < Duration::from_secs(MAX_RECORDING_SECONDS)
    {
        let read =
            unsafe { snd_pcm_readi(handle, frames.as_mut_ptr() as *mut c_void, frames.len()) };
        if read < 0 {
            let recovered = unsafe { snd_pcm_recover(handle, read as c_int, 1) };
            if recovered < 0 {
                unsafe { snd_pcm_close(handle) };
                return Err(format!("ALSA 录音读取失败（错误 {recovered}）"));
            }
            continue;
        }
        let bytes = read as usize * std::mem::size_of::<i16>();
        let raw = unsafe { std::slice::from_raw_parts(frames.as_ptr() as *const u8, bytes) };
        pcm.extend_from_slice(raw);
        if pcm.len() >= max_bytes {
            break;
        }
    }
    unsafe { snd_pcm_close(handle) };
    pcm.truncate(max_bytes);
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::pcm_to_wav;

    #[test]
    fn wav_header_matches_pcm_payload() {
        let wav = pcm_to_wav(&[1, 2, 3, 4], 16_000, 1, 16);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[40..44], &(4u32.to_le_bytes()));
        assert_eq!(&wav[44..], &[1, 2, 3, 4]);
    }
}
