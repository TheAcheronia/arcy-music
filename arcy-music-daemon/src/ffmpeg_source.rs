use rodio::Source;
use std::io::Read;
use std::process::{Command, Child, Stdio};
use std::time::Duration;

pub struct FFmpegSource {
    url: String,
    child: Child,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl FFmpegSource {
    pub fn new(url: &str) -> std::io::Result<Self> {
        let mut child = Command::new("ffmpeg")
            .args(&[
                "-hide_banner", "-loglevel", "error",
                "-probesize", "32768", "-analyzeduration", "0",
                "-reconnect", "1", "-reconnect_streamed", "1", "-reconnect_delay_max", "5", // Fix HTTP connection drops
                "-i", url,
                "-f", "f32le",
                "-ac", "2",
                "-ar", "44100",
                "pipe:1"
            ])
            .stdout(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().unwrap();
        let buf_reader = std::io::BufReader::with_capacity(1024 * 64, stdout);
        Ok(Self { url: url.to_string(), child, stdout: buf_reader })
    }
}

impl Iterator for FFmpegSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let mut sample_buf = [0u8; 4];
        if self.stdout.read_exact(&mut sample_buf).is_ok() {
            Some(f32::from_le_bytes(sample_buf))
        } else {
            None
        }
    }
}

impl Source for FFmpegSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> std::num::NonZero<u16> {
        std::num::NonZero::new(2).unwrap()
    }

    fn sample_rate(&self) -> std::num::NonZero<u32> {
        std::num::NonZero::new(44100).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let secs = pos.as_secs_f64();
        let mut child = Command::new("ffmpeg")
            .args(&[
                "-hide_banner", "-loglevel", "error",
                "-ss", &format!("{:.3}", secs),
                "-probesize", "32768", "-analyzeduration", "0",
                "-reconnect", "1", "-reconnect_streamed", "1", "-reconnect_delay_max", "5",
                "-i", &self.url,
                "-f", "f32le",
                "-ac", "2",
                "-ar", "44100",
                "pipe:1"
            ])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| rodio::source::SeekError::Other(std::sync::Arc::new(e)))?;

        if let Some(stdout) = child.stdout.take() {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.child = child;
            self.stdout = std::io::BufReader::with_capacity(1024 * 64, stdout);
            Ok(())
        } else {
            Err(rodio::source::SeekError::Other(std::sync::Arc::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to capture ffmpeg stdout",
            ))))
        }
    }
}

impl Drop for FFmpegSource {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
