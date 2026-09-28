//! Private mpv process and JSON IPC. mpv draws video; its OSC and key actions are disabled.
use serde_json::{json, Value};
use std::{collections::VecDeque, fs, io::{self, Read, Write}, os::unix::{fs::PermissionsExt, net::UnixStream}, path::{Path, PathBuf}, process::{Child, Command, Stdio}, time::{Duration, Instant}};

pub struct Session {
    child: Child,
    socket: PathBuf,
    ipc: Option<UnixStream>,
    input: Vec<u8>,
    source: Option<String>,
    resume: f64,
    created: Instant,
    pub position: f64,
    pub duration: f64,
    pub ready: bool,
    pub paused: bool,
    pub ended: bool,
    pub failed: bool,
    pub seeking: bool,
    pub hdr: bool,
    pub hardware: String,
    keys: VecDeque<String>,
    overlay: PathBuf,
    overlay_pending: bool,
    pub overlays: u64,
    tracks: Vec<(String, i64)>,
    selection: (i32, i32),
    overlay_rect: (i32,i32,i32,i32),
}

impl Session {
    pub fn launch(root: &Path, source: String, resume: f64) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        let socket = root.join(format!("mpv-{}.sock", std::process::id()));
        let overlay = root.join(format!("hud-{}.bgra", std::process::id()));
        let script = root.join("controls.lua");
        fs::write(&script, include_str!("../../controls.lua"))?;
        let mut cmd = Command::new("flatpak");
        cmd.args(["run", "--die-with-parent", "--filesystem=host-os:ro", "--filesystem=xdg-run/gamescope-0"])
            .arg(format!("--filesystem={}", root.display()))
            .args(["--env=GAMESCOPE_WAYLAND_DISPLAY=gamescope-0", "--env=ENABLE_GAMESCOPE_WSI=1", "--env=STEAM_GAMESCOPE_HDR_SUPPORTED=1"])
            .arg(format!("--env=VK_LAYER_PATH={}", std::env::var("PLXNATIVE_VULKAN_DIR").unwrap_or_else(|_| format!("{}/.local/share/plxnative-steamdeck/vulkan", std::env::var("HOME").unwrap_or_default()))))
            .args(["--env=VK_INSTANCE_LAYERS=VK_LAYER_FROG_gamescope_wsi_x86_64", "io.mpv.Mpv", "--no-config", "--idle=yes", "--force-window=yes", "--fullscreen", "--vo=gpu-next", "--gpu-api=vulkan", "--gpu-context=x11vk", "--target-colorspace-hint=yes", "--hwdec=auto", "--osc=no", "--osd-level=0", "--input-default-bindings=no", "--input-terminal=no", "--terminal=no", "--keep-open=no"])
            .arg(format!("--input-ipc-server={}", socket.display()))
            .arg(format!("--script={}", script.display()))
            .env("DISPLAY", ":1")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if std::env::var_os("PLXNATIVE_MPV_TEST_LOG").is_some() {
            // Opt-in fixture diagnostic, inside the private IPC directory; launcher leaves it off.
            cmd.arg(format!("--log-file={}", root.join("mpv-test.log").display())).arg("--msg-level=all=debug");
        }
        // No URL or token in argv, logs, script or manifest: loadfile goes over private IPC.
        let child = cmd.spawn()?;
        Ok(Self { child, socket, ipc: None, input: Vec::new(), source: Some(source), resume, created: Instant::now(), position: resume, duration: 0., ready: false, paused: false, ended: false, failed: false, seeking: false, hdr: false, hardware: String::new(), keys: VecDeque::new(), overlay, overlay_pending: false, overlays: 0, tracks: Vec::new(), selection: (-2,-2), overlay_rect: (0,0,1280,720) })
    }

    pub fn command(&mut self, command: Value) -> bool {
        self.send(json!({"command": command}))
    }

    fn send(&mut self, value: Value) -> bool {
        let Some(ipc) = self.ipc.as_mut() else { return false };
        let mut bytes = value.to_string().into_bytes(); bytes.push(b'\n');
        if ipc.write_all(&bytes).is_err() { self.failed = true; return false; }
        true
    }

    pub fn tick(&mut self) {
        if self.failed { return; }
        if self.child.try_wait().ok().flatten().is_some() { self.failed = !self.ended; return; }
        if self.ipc.is_none() {
            match UnixStream::connect(&self.socket) {
                Ok(ipc) => {
                    if ipc.set_nonblocking(true).is_err() { self.failed = true; return; }
                    self.ipc = Some(ipc);
                    self.command(json!(["request_log_messages", "debug"]));
                    for (id, prop) in ["time-pos", "duration", "pause", "seeking", "hwdec-current", "track-list"].iter().enumerate() {
                        self.command(json!(["observe_property", id + 1, prop]));
                    }
                    if let Some(source) = self.source.take() {
                        self.command(json!(["loadfile", source, "replace", -1, {"start": self.resume.to_string()}]));
                    }
                }
                Err(_) if self.created.elapsed() < Duration::from_secs(15) => return,
                Err(_) => { self.failed = true; return; }
            }
        }
        let mut bytes = [0; 8192];
        loop {
            match self.ipc.as_mut().unwrap().read(&mut bytes) {
                Ok(0) => { self.failed = !self.ended; break; }
                Ok(n) => self.input.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => { self.failed = true; break; }
            }
            if self.input.len() > 1024 * 1024 { self.failed = true; break; }
        }
        while let Some(end) = self.input.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.input.drain(..=end).collect();
            if let Ok(event) = serde_json::from_slice::<Value>(&line) { self.event(&event); }
        }
    }

    fn event(&mut self, event: &Value) {
        if event["request_id"] == 100 { self.overlay_pending = false; if event["error"] == "success" { self.overlays += 1; } }
        match event["event"].as_str() {
            Some("log-message") => {
                let text = event["text"].as_str().unwrap_or("");
                if text.contains("Picked surface configuration") { self.hdr = text.contains("VK_COLOR_SPACE_HDR10_ST2084_EXT"); }
            }
            Some("playback-restart") => { self.ready = true; self.seeking = false; }
            Some("end-file") => { if event["reason"] == "eof" { self.ended = true; } else if event["reason"] == "error" { self.failed = true; } }
            Some("property-change") => match event["name"].as_str() {
                Some("time-pos") => if let Some(v) = event["data"].as_f64() { self.position = v; },
                Some("duration") => if let Some(v) = event["data"].as_f64() { self.duration = v; },
                Some("pause") => self.paused = event["data"].as_bool().unwrap_or(false),
                Some("seeking") => self.seeking = event["data"].as_bool().unwrap_or(false),
                Some("hwdec-current") => self.hardware = event["data"].as_str().unwrap_or("").into(),
                Some("track-list") => {
                    self.tracks = event["data"].as_array().into_iter().flatten().filter_map(|t| Some((t["type"].as_str()?.into(), t["id"].as_i64()?))).collect();
                }
                _ => {}
            },
            Some("client-message") if event["args"][0] == "plx-key" => {
                if let Some(key) = event["args"][1].as_str().filter(|k| ["left", "right", "up", "down", "ok", "back", "playpause"].contains(k)) {
                    if self.keys.len() < 32 { self.keys.push_back(key.into()); }
                }
            }
            Some("client-message") if event["args"][0] == "plx-click" => {
                let x = event["args"][1].as_str().and_then(|s| s.parse::<f64>().ok());
                let y = event["args"][2].as_str().and_then(|s| s.parse::<f64>().ok());
                if let (Some(x),Some(y)) = (x,y) {
                    let (ox,oy,w,h) = self.overlay_rect;
                    if x >= ox as f64 && y >= oy as f64 && x < (ox+w) as f64 && y < (oy+h) as f64 && self.keys.len()<32 {
                        self.keys.push_back(format!("ck:{},{}", ((x-ox as f64)*1920./w as f64) as i32, ((y-oy as f64)*1080./h as f64) as i32));
                    }
                }
            }
            _ => {}
        }
    }

    pub fn keys(&mut self) -> Vec<String> { self.keys.drain(..).collect() }
    pub fn select_tracks(&mut self, audio: i32, subtitle: i32) {
        if !self.ready || self.tracks.is_empty() { return; }
        for (kind, property, ordinal, old) in [("audio", "aid", audio, self.selection.0), ("sub", "sid", subtitle, self.selection.1)] {
            if ordinal == old { continue; }
            let value = if ordinal < 0 { json!(if kind == "audio" { "auto" } else { "no" }) }
                else if let Some((_, id)) = self.tracks.iter().filter(|(k,_)| k == kind).nth(ordinal as usize) { json!(id) }
                else { continue; };
            if self.command(json!(["set_property", property, value])) {
                if kind == "audio" { self.selection.0 = ordinal; } else { self.selection.1 = ordinal; }
            }
        }
    }
    pub fn accepts_overlay(&self) -> bool { self.ready && !self.overlay_pending && !self.failed }
    pub fn overlay(&mut self, x: i32, y: i32, width: i32, height: i32, bgra: &[u8]) {
        if !self.accepts_overlay() || width <= 0 || height <= 0 || bgra.len() != width as usize * height as usize * 4 { return; }
        self.overlay_rect = (x,y,width,height);
        if fs::write(&self.overlay, bgra).is_err() { self.failed = true; return; }
        self.overlay_pending = self.send(json!({"command": ["overlay-add", 1, x, y, self.overlay, 0, "bgra", width, height, width * 4], "request_id": 100}));
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.command(json!(["quit"]));
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() { break; }
            std::thread::sleep(Duration::from_millis(5));
        }
        // Flatpak's die-with-parent also tears down the sandbox if the native UI crashes.
        let _ = self.child.kill(); let _ = self.child.wait();
        let _ = fs::remove_file(&self.socket); let _ = fs::remove_file(&self.overlay);
    }
}
