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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputDeviceKind {
    /// Dispositivo virtual criado pelo proprio daemon.
    #[default]
    Virtual,
    /// Um device de saida existente, escolhido por nome.
    Named(String),
    /// Default do sistema.
    Default,
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
    /// Nome da fonte virtual (microfone) para Discord/Slack/Meet.
    ///
    /// Vazio desliga. Quando ligado, o audio do sink aparece como microfone
    /// nesses apps.
    pub virtual_mic: Option<String>,
    /// Descricao mostrada na lista de microfones.
    pub virtual_mic_description: String,
    /// Microfone real para ser somado aos efeitos na fonte virtual.
    ///
    /// Com isso o microfone virtual entrega **voz + efeitos** juntos, e o
    /// usuario escolhe um unico microfone no Discord/Slack/Meet em vez de
    /// ficar trocando de dispositivo. Vazio desliga.
    ///
    /// Cuidado: rotear para o *sink* (em vez da fonte) faz os efeitos
    /// silenciarem. Ver `route_mic_into_sink`.
    pub mic_into_sink: Option<String>,
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
            virtual_mic: Some("StreamDeckSoundBarMic".into()),
            virtual_mic_description: "SoundEffectsStreamDeckMic".into(),
            mic_into_sink: None,
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
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(ConfigError::Serialize)?,
        )?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Home real do usuario.
    ///
    /// Dentro do sandbox do Flatpak, `$HOME` aponta para `.var/app/<id>/`,
    /// e o plugin procuraria o daemon no lugar errado. `/etc/passwd` traz o
    /// home de verdade, entao usamos ele quando o path parece um sandbox.
    pub fn real_home() -> PathBuf {
        let home_env = std::env::var("HOME").unwrap_or_default();

        // Path de sandbox: termina em /.var/app/<id>/ ou /.local/share/flatpak/...
        let is_sandbox =
            home_env.contains("/.var/app/") || home_env.contains("/.local/share/flatpak/");

        if !is_sandbox {
            return PathBuf::from(&home_env);
        }

        // Procura o home real no passwd pelo nome de usuario logado.
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_default();

        if !user.is_empty() {
            if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
                for line in passwd.lines() {
                    let mut parts = line.split(':');
                    if parts.next() == Some(user.as_str()) {
                        if let Some(h) = parts.nth(4) {
                            if !h.is_empty() {
                                return PathBuf::from(h);
                            }
                        }
                    }
                }
            }
        }

        // Ultimo recurso: tira o sufixo do sandbox.
        if let Some(idx) = home_env.find("/.var/app/") {
            return PathBuf::from(&home_env[..idx]);
        }
        PathBuf::from(&home_env)
    }

    /// Diretorio padrao de configuracao por plataforma.
    pub fn default_dir() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                return PathBuf::from(appdata).join("soundbar-streamdeck");
            }
        }

        let home = Config::real_home();

        // `XDG_CONFIG_HOME` e respeitado, mas nunca quando aponta para dentro
        // do sandbox do Flatpak: o daemon roda no host, em `~/.config`.
        let is_sandbox = home.as_os_str().is_empty()
            || std::env::var("HOME")
                .map(|h| h.contains("/.var/app/"))
                .unwrap_or(false);

        if !is_sandbox {
            if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
                if !xdg.is_empty() && !xdg.contains("/.var/app/") {
                    return PathBuf::from(xdg).join("soundbar-streamdeck");
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            home.join("Library/Application Support/soundbar-streamdeck")
        }
        #[cfg(not(target_os = "macos"))]
        {
            home.join(".config").join("soundbar-streamdeck")
        }
    }

    /// Descobre o diretorio de configuracao a usar.
    ///
    /// Ordem de precedencia:
    ///   1. `SOUNDBAR_CONFIG_DIR`, se definido (o `--config` tem prioridade
    ///      e e resolvido pelo binario antes de chamar esta funcao).
    ///   2. Modo portatil: a pasta do proprio executavel, se ela contiver
    ///      `sounds/` ou `config.json`. Isso deixa o daemon autocontido.
    ///   3. A pasta do plugin instalado (`~/.config/streamdeck/plugins/
    ///      SoundEffectsStreamDeck.sdPlugin`), se ela existir. Serve para a
    ///      CLI, que fica em outro lugar mas precisa achar os mesmos dados.
    ///   4. O diretorio padrao da plataforma.
    pub fn resolve_dir() -> PathBuf {
        if let Ok(d) = std::env::var("SOUNDBAR_CONFIG_DIR") {
            if !d.trim().is_empty() {
                return PathBuf::from(d);
            }
        }
        // Instalacao portatil (o binario ao lado de um `sounds/`).
        if let Some(dir) = portable_dir() {
            return dir;
        }

        // O diretorio padrao e sempre o certo: o daemon e o socket vivem
        // nele. A pasta do plugin NAO serve aqui, mesmo que exista, porque
        // o plugin e apenas um cliente do daemon.
        Config::default_dir()
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

/// Pasta do executavel, se ela existir e tiver marcas de modo portatil.
fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();

    // So considera a pasta do executavel como config se ela tiver marcas
    // de uso real. Um plugin instalado em ~/.config/opendeck/plugins/... tem
    // manifest.json e assets, mas nao e o diretorio de configuracao: sem
    // esta checagem o daemon e procurado no lugar errado.
    // Exigimos `sounds/`: e a marca de uma instalacao portatil real. Um
    // plugin instalado numa pasta de plugins tem arquivos como `manifest.json`
    // ou um `config.json` de outra ferramenta, e usaria a pasta por engano.
    if dir.join("sounds").is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Procura a pasta do plugin instalado nos caminhos usuais de cada plataforma.
///
/// Usado pela CLI, que roda de `~/.local/bin` e nao consegue detectar o modo
/// portatil pelo proprio executavel.
pub fn plugin_dir() -> Option<PathBuf> {
    let home = Config::real_home();

    #[cfg(target_os = "linux")]
    let candidates = [
        // OpenDeck nativo (o Flatpak nao tem acesso ao host; ver README).
        PathBuf::from(&home).join(".config/opendeck/plugins/com.soundbar.streamdeck.sdPlugin"),
        PathBuf::from(&home).join(".config/streamdeck/plugins/SoundEffectsStreamDeck.sdPlugin"),
        PathBuf::from(&home)
            .join(".local/share/StreamDeck/plugins/SoundEffectsStreamDeck.sdPlugin"),
        PathBuf::from(&home)
            .join(".config/Elgato/StreamDeck/plugins/SoundEffectsStreamDeck.sdPlugin"),
    ];

    #[cfg(target_os = "macos")]
    let candidates = [PathBuf::from(&home)
        .join("Library/Application Support/StreamDeck/Plugins/SoundEffectsStreamDeck.sdPlugin")];

    #[cfg(target_os = "windows")]
    let candidates = [PathBuf::from(&home)
        .join("AppData/Roaming/Elgato/StreamDeck/Plugins/SoundEffectsStreamDeck.sdPlugin")];

    candidates.into_iter().find(|c| c.is_dir())
}
