//! Configuracao persistente do soundbar.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// Erros de leitura/escrita da configuracao.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    Serialize(serde_json::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "erro de io na configuracao: {e}"),
            ConfigError::Parse(e) => write!(f, "configuracao invalida: {e}"),
            ConfigError::Serialize(e) => write!(f, "falha ao serializar configuracao: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self {
        ConfigError::Parse(e)
    }
}

/// Para onde o audio e enviado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputDeviceKind {
    /// Dispositivo virtual criado pelo proprio daemon.
    Virtual,
    /// Um device de saida existente, escolhido por nome.
    Named(String),
    /// Default do sistema.
    Default,
}

impl Default for OutputDeviceKind {
    fn default() -> Self {
        OutputDeviceKind::Virtual
    }
}

/// Configuracao de audio e caminhos.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Nome do device virtual anunciado ao sistema.
    pub virtual_device: String,
    /// Nome do device virtual para o sistema operacional.
    pub virtual_device_description: String,
    /// Destino final do audio.
    pub output: OutputDeviceKind,
    /// Ganho global linear (1.0 = unity).
    pub master_gain: f32,
    /// Limita o polyphony simultaneo. 0 = ilimitado.
    pub max_polyphony: usize,
    /// Nome do sink do Pulse/pipewire a ser capturado como entrada de monitor.
    pub monitor_source: Option<String>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig {
            virtual_device: "StreamDeckSoundBar".into(),
            virtual_device_description: "Sound Effects Stream Deck Output".into(),
            output: OutputDeviceKind::default(),
            master_gain: 1.0,
            max_polyphony: 16,
            monitor_source: None,
        }
    }
}

/// Configuracao completa.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub audio: AudioConfig,
    /// Diretorio de onde os efeitos sao carregados.
    pub sounds_dir: Option<PathBuf>,
    /// Arquivo de configuracao do Stream Deck em formato JSON.
    pub deck_layout: Option<PathBuf>,
}

impl Config {
    /// Carrega a configuracao de um diretorio, criando os padroes se necessario.
    pub fn load_dir(dir: &Path) -> Result<Self, ConfigError> {
        let path = dir.join("config.json");
        if !path.exists() {
            let cfg = Config {
                audio: AudioConfig::default(),
                sounds_dir: Some(dir.join("sounds")),
                deck_layout: Some(dir.join("layout.json")),
            };
            cfg.save_dir(dir)?;
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Salva a configuracao em `dir/config.json`.
    pub fn save_dir(&self, dir: &Path) -> Result<(), ConfigError> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("config.json");
        let tmp = dir.join("config.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(ConfigError::Serialize)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Diretorio padrao de configuracao por plataforma.
    pub fn default_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("soundbar-streamdeck");
            }
        }
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                return PathBuf::from(appdata).join("soundbar-streamdeck");
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        #[cfg(target_os = "macos")]
        {
            return PathBuf::from(&home)
                .join("Library/Application Support/soundbar-streamdeck");
        }
        #[cfg(not(target_os = "macos"))]
        PathBuf::from(home).join(".config").join("soundbar-streamdeck")
    }

    /// Valida e corrige valores impossiveis.
    pub fn sanitize(&mut self) {
        self.audio.master_gain = self.audio.master_gain.clamp(0.0, 4.0);
        self.audio.max_polyphony = self.audio.max_polyphony.min(64);
        if self.audio.virtual_device.trim().is_empty() {
            self.audio.virtual_device = AudioConfig::default().virtual_device;
        }
    }
}
