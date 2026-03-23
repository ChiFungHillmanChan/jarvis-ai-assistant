use tokio::process::Command;

#[derive(Clone)]
pub struct TextToSpeech {
    voice: String,
    rate: u32,
    enabled: bool,
}

impl TextToSpeech {
    pub fn new() -> Self {
        TextToSpeech {
            voice: "Samantha".to_string(),
            rate: 200,
            enabled: true,
        }
    }

    pub fn set_voice(&mut self, voice: String) { self.voice = voice; }
    pub fn set_rate(&mut self, rate: u32) { self.rate = rate; }
    pub fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    pub async fn speak(&self, text: &str) -> Result<(), String> {
        if !self.enabled || text.is_empty() { return Ok(()); }
        let status = Command::new("say")
            .arg("-v").arg(&self.voice)
            .arg("-r").arg(self.rate.to_string())
            .arg(text)
            .status().await.map_err(|e| format!("TTS error: {}", e))?;
        if !status.success() { return Err("TTS command failed".to_string()); }
        Ok(())
    }

    pub async fn list_voices() -> Result<Vec<String>, String> {
        let output = Command::new("say").arg("-v").arg("?")
            .output().await.map_err(|e| format!("Failed to list voices: {}", e))?;
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.lines().filter_map(|line| line.split_whitespace().next().map(|s| s.to_string())).collect())
    }
}
