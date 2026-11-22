//! Recording one segment: the Roblox window's picture (ffmpeg's Windows Graphics Capture source, hardware H.264
//! when there is one) and Roblox's own sound (WASAPI process loopback, so no other app's sound and no microphone),
//! joined into one MP4 when the segment stops. Only the Roblox window is captured: the overlay, other windows and
//! notifications never end up in a video.

use anyhow::{anyhow, bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ ffmpeg

/// ffmpeg 8 or newer (it has the window capture source): ours in the tools folder, else one on PATH.
pub fn ffmpeg(tools: &Path) -> Option<PathBuf> {
    let ours = tools.join("ffmpeg.exe");
    if ours.is_file() {
        return Some(ours);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join("ffmpeg.exe")).find(|p| p.is_file() && has_capture(p))
}

fn has_capture(ffmpeg: &Path) -> bool {
    roxai::util::run_capture(&ffmpeg.to_string_lossy(), &["-hide_banner", "-filters"], Duration::from_secs(20))
        .is_ok_and(|(_, out)| out.contains("gfxcapture"))
}

/// Download ffmpeg (a full Windows build from the BtbN/FFmpeg-Builds GitHub releases) into the tools folder.
pub fn install_ffmpeg(tools: &Path, report: roxai::deps::Report) -> Result<PathBuf> {
    std::fs::create_dir_all(tools)?;
    let url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";
    let zip_path = roxai::deps::download(url, "rox-research-ffmpeg.zip", "ffmpeg (video recorder)", report)?;
    report("Unpacking ffmpeg…", None);
    let mut z = zip::ZipArchive::new(std::fs::File::open(&zip_path)?).context("the ffmpeg download wasn't a valid zip")?;
    let mut found = false;
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let name = f.name().rsplit('/').next().unwrap_or("").to_string();
        if name == "ffmpeg.exe" || name == "ffprobe.exe" {
            let tmp = tools.join(format!("{name}.new"));
            let mut out = std::fs::File::create(&tmp)?;
            std::io::copy(&mut f, &mut out)?;
            drop(out);
            std::fs::rename(&tmp, tools.join(&name))?;
            found |= name == "ffmpeg.exe";
        }
    }
    let _ = std::fs::remove_file(&zip_path);
    if !found {
        bail!("ffmpeg.exe wasn't in the download");
    }
    Ok(tools.join("ffmpeg.exe"))
}

/// The best H.264 encoder that works on this PC (graphics card first: recording must not slow the game down).
fn encoder(ffmpeg: &Path) -> &'static str {
    static PICK: OnceLock<&'static str> = OnceLock::new();
    PICK.get_or_init(|| {
        for enc in ["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf"] {
            let ok = roxai::util::run_capture(
                &ffmpeg.to_string_lossy(),
                &["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=30", "-t", "0.3", "-c:v", enc, "-f", "null", "-"],
                Duration::from_secs(30),
            )
            .is_ok_and(|(code, _)| code == 0);
            if ok {
                return enc;
            }
        }
        "libx264"
    })
}

/// A video's length, read from ffmpeg's own report.
fn video_seconds(ffmpeg: &str, file: &str) -> Option<f64> {
    let (_, out) = roxai::util::run_capture(ffmpeg, &["-hide_banner", "-i", file], Duration::from_secs(60)).ok()?;
    let d = out.split("Duration:").nth(1)?.split(',').next()?.trim().to_string();
    let p: Vec<f64> = d.split(':').filter_map(|x| x.parse().ok()).collect();
    (p.len() == 3).then(|| p[0] * 3600.0 + p[1] * 60.0 + p[2])
}

// ------------------------------------------------------------------ the Roblox window

#[derive(Clone, Copy, Debug)]
pub struct GameWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub width: i32,
    pub height: i32,
}

/// The visible Roblox game window (the biggest window of a Roblox process), if a game is open.
#[cfg(windows)]
pub fn roblox_window() -> Option<GameWindow> {
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible};

    fn exe_name(pid: u32) -> String {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return String::new();
            }
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            if ok == 0 {
                return String::new();
            }
            String::from_utf16_lossy(&buf[..len as usize]).rsplit('\\').next().unwrap_or("").to_lowercase()
        }
    }
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp as *mut Vec<GameWindow>);
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut r);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w < 200 || h < 150 {
            return 1;
        }
        let exe = exe_name(pid);
        // ROXRESEARCH_TEST_EXE lets checks record another app's window when Roblox isn't open
        let test = std::env::var("ROXRESEARCH_TEST_EXE").unwrap_or_default().to_lowercase();
        if exe.starts_with("robloxplayer") || exe == "windows10universal.exe" || (!test.is_empty() && exe == test) {
            out.push(GameWindow { hwnd: hwnd as isize, pid, width: w, height: h });
        }
        1
    }
    let mut found: Vec<GameWindow> = vec![];
    unsafe {
        EnumWindows(Some(each), &mut found as *mut _ as LPARAM);
    }
    found.into_iter().max_by_key(|w| w.width as i64 * w.height as i64)
}

#[cfg(not(windows))]
pub fn roblox_window() -> Option<GameWindow> {
    None
}

// ------------------------------------------------------------------ game sound

/// Records one process's sound (and its child processes') to a 48 kHz stereo float WAV, padding silence so the
/// sound stays in step with the video while the game is quiet.
struct Audio {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Result<u64>>>,
}

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;

fn wav_header(data_bytes: u32) -> Vec<u8> {
    let mut h = Vec::with_capacity(44);
    let block = CHANNELS as u32 * 4;
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    h.extend_from_slice(&CHANNELS.to_le_bytes());
    h.extend_from_slice(&RATE.to_le_bytes());
    h.extend_from_slice(&(RATE * block).to_le_bytes());
    h.extend_from_slice(&(block as u16).to_le_bytes());
    h.extend_from_slice(&32u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data_bytes.to_le_bytes());
    h
}

#[cfg(windows)]
fn audio_loop(pid: u32, path: PathBuf, stop: Arc<AtomicBool>, started: Instant) -> Result<u64> {
    use std::collections::VecDeque;
    use std::io::{Seek, SeekFrom};
    use wasapi::*;
    let _ = initialize_mta();
    let mut file = std::fs::File::create(&path)?;
    file.write_all(&wav_header(0))?;
    let block = CHANNELS as usize * 4;
    let mut written: u64 = 0; // frames
    let write = |file: &mut std::fs::File, bytes: &[u8], written: &mut u64| -> Result<()> {
        file.write_all(bytes)?;
        *written += (bytes.len() / block) as u64;
        Ok(())
    };
    let capture = (|| -> std::result::Result<_, Box<dyn std::error::Error>> {
        let format = WaveFormat::new(32, 32, &SampleType::Float, RATE as usize, CHANNELS as usize, None);
        let mut client = AudioClient::new_application_loopback_client(pid, true)?;
        client.initialize_client(&format, 0, &Direction::Capture, &ShareMode::Shared, true)?;
        let event = client.set_get_eventhandle()?;
        let cap = client.get_audiocaptureclient()?;
        client.start_stream()?;
        Ok((client, event, cap))
    })();
    let (client, event, cap) = match capture {
        Ok(c) => (Some(c.0), Some(c.1), Some(c.2)),
        Err(e) => {
            roxai::warn!("recorder: no game sound ({e}); the video will be silent");
            (None, None, None)
        }
    };
    let mut queue: VecDeque<u8> = VecDeque::new();
    while !stop.load(Ordering::Relaxed) {
        let mut got = false;
        if let (Some(event), Some(cap)) = (&event, &cap) {
            let _ = event.wait_for_event(50);
            loop {
                match cap.get_next_nbr_frames() {
                    Ok(Some(n)) if n > 0 => {
                        if cap.read_from_device_to_deque(&mut queue).is_err() {
                            break;
                        }
                        got = true;
                    }
                    _ => break,
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(50));
        }
        if got {
            let n = queue.len() / block * block;
            let bytes: Vec<u8> = queue.drain(..n).collect();
            write(&mut file, &bytes, &mut written)?;
        }
        // quiet game: no packets arrive, so fill the gap with silence to keep step with the video
        let due = (started.elapsed().as_secs_f64() * RATE as f64) as u64;
        if !got && due > written + RATE as u64 / 10 {
            let gap = (due - written) as usize;
            write(&mut file, &vec![0u8; gap * block], &mut written)?;
        }
    }
    if let Some(c) = &client {
        let _ = c.stop_stream();
    }
    let bytes = (written as usize * block) as u32;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&wav_header(bytes))?;
    Ok(written)
}

impl Audio {
    fn start(pid: u32, path: PathBuf, started: Instant) -> Audio {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        #[cfg(windows)]
        let thread = Some(std::thread::spawn(move || audio_loop(pid, path, s2, started)));
        #[cfg(not(windows))]
        let thread = {
            let _ = (pid, path, s2, started);
            None
        };
        Audio { stop, thread }
    }
    /// Stop; returns the seconds of sound written.
    fn finish(mut self) -> Result<f64> {
        self.stop.store(true, Ordering::Relaxed);
        match self.thread.take() {
            Some(t) => Ok(t.join().map_err(|_| anyhow!("the sound recorder crashed"))?? as f64 / RATE as f64),
            None => bail!("no sound recorder"),
        }
    }
}

// ------------------------------------------------------------------ one segment

pub struct Recording {
    video: Child,
    audio: Audio,
    video_tmp: PathBuf,
    audio_tmp: PathBuf,
    out: PathBuf,
    started: Instant,
    ffmpeg: PathBuf,
}

/// What's recording right now (one segment at a time).
pub fn current() -> &'static Mutex<Option<(String, usize, Recording)>> {
    static REC: OnceLock<Mutex<Option<(String, usize, Recording)>>> = OnceLock::new();
    REC.get_or_init(|| Mutex::new(None))
}

pub fn start(ffmpeg: &Path, out: &Path) -> Result<Recording> {
    let win = roblox_window().ok_or_else(|| anyhow!("Open the game in Roblox first (the Roblox window must be open, not minimized)."))?;
    let dir = out.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let stem = out.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let video_tmp = dir.join(format!("{stem}.video.mp4"));
    let audio_tmp = dir.join(format!("{stem}.audio.wav"));
    let enc = encoder(ffmpeg);
    let src = format!("gfxcapture=hwnd={}:max_framerate=30,hwdownload,format=bgra", win.hwnd);
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", &src, "-vf", "scale=w='min(1920,iw)':h='min(1080,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2:flags=bicubic,format=nv12", "-r", "30", "-c:v", enc]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if enc == "libx264" {
        args.extend(["-preset", "veryfast", "-crf", "23"].map(String::from));
    } else {
        args.extend(["-b:v", "6M"].map(String::from));
    }
    args.push(video_tmp.to_string_lossy().into());
    let started = Instant::now();
    let audio = Audio::start(win.pid, audio_tmp.clone(), started);
    let video = roxai::util::hidden_command(ffmpeg)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't start ffmpeg")?;
    roxai::info!("recorder: {} ({}x{}, {enc}) → {}", win.hwnd, win.width, win.height, out.display());
    Ok(Recording { video, audio, video_tmp, audio_tmp, out: out.to_path_buf(), started, ffmpeg: ffmpeg.to_path_buf() })
}

impl Recording {
    pub fn seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Is ffmpeg still recording? (It stops by itself if the Roblox window closes.)
    pub fn alive(&mut self) -> bool {
        matches!(self.video.try_wait(), Ok(None))
    }

    /// Stop, join picture and sound into the segment's MP4. Returns its length in seconds.
    pub fn finish(mut self) -> Result<u64> {
        let secs = self.seconds();
        if let Some(mut stdin) = self.video.stdin.take() {
            let _ = stdin.write_all(b"q");
        }
        let t = Instant::now();
        loop {
            match self.video.try_wait()? {
                Some(_) => break,
                None if t.elapsed() > Duration::from_secs(15) => {
                    let _ = self.video.kill();
                    let _ = self.video.wait();
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let mut err = String::new();
        if let Some(mut e) = self.video.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err);
        }
        let audio_secs = self.audio.finish().ok();
        let ff = self.ffmpeg.to_string_lossy().to_string();
        let (v, a, o) = (self.video_tmp.to_string_lossy().to_string(), self.audio_tmp.to_string_lossy().to_string(), self.out.to_string_lossy().to_string());
        // no picture at all (the game was minimized, or capture failed): don't save an empty segment
        let has_picture = self.video_tmp.is_file() && video_seconds(&ff, &v).is_some_and(|s| s > 0.5);
        if !has_picture {
            let _ = std::fs::remove_file(&self.video_tmp);
            let _ = std::fs::remove_file(&self.audio_tmp);
            let why = err.lines().last().map(|l| format!(" ({l})")).unwrap_or_default();
            bail!("Nothing was recorded{why}. Keep the Roblox window open on screen (not minimized) while you record, then record this segment again.");
        }
        // the sound starts before ffmpeg's first frame; both stop together, so skip the difference at the start
        let skip = match (audio_secs, video_seconds(&ff, &v)) {
            (Some(a), Some(vs)) if a > vs => format!("{:.3}", (a - vs).min(5.0)),
            _ => "0".into(),
        };
        let joined = audio_secs.is_some()
            && self.audio_tmp.is_file()
            && roxai::util::run_capture(&ff, &["-hide_banner", "-loglevel", "error", "-y", "-i", &v, "-ss", &skip, "-i", &a, "-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-shortest", &o], Duration::from_secs(600))
                .is_ok_and(|(code, _)| code == 0);
        if joined {
            let _ = std::fs::remove_file(&self.video_tmp);
        } else {
            roxai::warn!("recorder: saving the segment without sound");
            std::fs::rename(&self.video_tmp, &self.out)?;
        }
        let _ = std::fs::remove_file(&self.audio_tmp);
        Ok(secs)
    }
}
