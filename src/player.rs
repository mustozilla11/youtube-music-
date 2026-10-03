use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const SOCKET: &str = "/tmp/ytmusic-mpv.sock";

/// Thin wrapper around an mpv process controlled via its JSON IPC socket.
pub struct Player {
    _proc: Child,
}

impl Player {
    pub fn new() -> Result<Self> {
        // Verify mpv is installed
        let check = Command::new("which").arg("mpv").output()?;
        if check.stdout.is_empty() {
            bail!("mpv bulunamadı. Lütfen kurun: sudo pacman -S mpv");
        }

        // Remove stale socket from a previous run
        let _ = std::fs::remove_file(SOCKET);

        let child = Command::new("mpv")
            .args([
                "--idle=yes",
                "--no-video",
                "--vo=null",
                "--no-audio-display",
                "--force-window=no",
                "--no-terminal",
                "--quiet",
                "--volume=70",
                &format!("--input-ipc-server={SOCKET}"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        // Wait up to 5 s for the socket to appear
        let start = std::time::Instant::now();
        while !std::path::Path::new(SOCKET).exists() {
            if start.elapsed() > Duration::from_secs(5) {
                bail!("mpv başlatılamadı (socket zaman aşımı)");
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        Ok(Self { _proc: child })
    }

    /// Send a JSON-IPC command and read back the first non-event response.
    fn cmd(&self, body: Value) -> Result<Value> {
        let mut s = UnixStream::connect(SOCKET)?;
        s.set_read_timeout(Some(Duration::from_millis(400)))?;

        let msg = format!("{}\n", serde_json::to_string(&body)?);
        s.write_all(msg.as_bytes())?;
        s.flush()?;

        let reader = BufReader::new(s);
        for line in reader.lines().flatten() {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                // mpv events have an "event" key; responses have "error"
                if v.get("error").is_some() {
                    return Ok(v);
                }
            }
        }
        Ok(json!({"error":"timeout"}))
    }

    pub fn load_url(&self, url: &str) -> Result<()> {
        self.cmd(json!({"command": ["loadfile", url]}))?;
        Ok(())
    }

    pub fn toggle_pause(&self) -> Result<()> {
        self.cmd(json!({"command": ["cycle", "pause"]}))?;
        Ok(())
    }

    pub fn set_volume(&self, v: f64) -> Result<()> {
        self.cmd(json!({"command": ["set_property", "volume", v]}))?;
        Ok(())
    }

    pub fn seek(&self, pos: f64) -> Result<()> {
        self.cmd(json!({"command": ["seek", pos, "absolute"]}))?;
        Ok(())
    }

    fn get_f64(&self, prop: &str) -> f64 {
        self.cmd(json!({"command": ["get_property", prop]}))
            .ok()
            .and_then(|v| v["data"].as_f64())
            .unwrap_or(0.0)
    }

    fn get_bool(&self, prop: &str) -> bool {
        self.cmd(json!({"command": ["get_property", prop]}))
            .ok()
            .and_then(|v| v["data"].as_bool())
            .unwrap_or(true)
    }

    /// Returns (position_secs, duration_secs, is_paused, is_idle, is_eof).
    pub fn get_state(&self) -> (f64, f64, bool, bool, bool) {
        let pos = self.get_f64("time-pos");
        let dur = self.get_f64("duration");
        let pause = self.get_bool("pause");
        let idle = self.get_bool("idle-active");
        let eof = self.cmd(json!({"command": ["get_property", "eof-reached"]}))
            .ok()
            .and_then(|v| v["data"].as_bool())
            .unwrap_or(false);

        (pos, dur, pause, idle, eof)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.cmd(json!({"command": ["quit"]}));
        let _ = std::fs::remove_file(SOCKET);
    }
}
