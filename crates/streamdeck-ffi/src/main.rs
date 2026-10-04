//! Plugin OpenDeck do Sound Effects Stream Deck.
//!
//! Roda como processo separado dentro do OpenDeck. Cada tecla configurada
//! chama o daemon local pelo socket IPC e o daemon cuida do audio.
//!
//! Nao ha acesso direto ao audio aqui: o plugin so envia mensagens.

use anyhow::Result;
use openaction::*;
use serde::{Deserialize, Serialize};
use soundbar_core::config::Config;
use soundbar_core::ipc::{Conn, Endpoint};
use soundbar_core::protocol::{ClientMessage, DaemonMessage, DeviceKind};

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Configuracao por tecla.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct PlaySettings {
    /// Id do efeito. Vazio =Discover via daemon.
    effect: String,
    /// Ganho do slot.
    gain: f32,
    /// Texto na tecla.
    label: String,
    /// Liga/desliga em vez de disparar.
    toggle: bool,
}

/// Cliente do daemon, com reconexao automatica.
struct Daemon {
    config_dir: std::path::PathBuf,
}

impl Daemon {
    fn new() -> Self {
        Daemon {
            config_dir: Config::resolve_dir(),
        }
    }

    fn endpoint(&self) -> Endpoint {
        Endpoint::from_config_dir(&self.config_dir)
    }

    /// Envia uma mensagem e le a resposta.
    fn request(&self, msg: &ClientMessage, _timeout: Duration) -> Result<Option<DaemonMessage>> {
        let mut conn = Conn::connect(&self.endpoint())
            .map_err(|e| anyhow::anyhow!("IPC falhou em {}: {e}", self.endpoint().display()))?;
        conn.write_line(&serde_json::to_string(msg)?)
            .map_err(|e| anyhow::anyhow!("envio falhou: {e}"))?;
        let Some(line) = conn.read_line()? else {
            log::warn!("daemon fechou sem responder");
            return Ok(None);
        };
        let reply: DaemonMessage = serde_json::from_str(&line)
            .map_err(|e| anyhow::anyhow!("resposta invalida '{line}': {e}"))?;

        // O daemon responde `error` em vez de falhar a conexao. Sem checar
        // isso, um efeito inexistente ou polyphony cheia passava por sucesso.
        if let DaemonMessage::Error { message } = &reply {
            return Err(anyhow::anyhow!("daemon: {message}"));
        }
        Ok(Some(reply))
    }

    /// Toca um efeito. Em modo toggle, o daemon devolve o id da instancia
    /// para poder parar na segunda tecla.
    async fn play(&self, effect: &str, gain: f32) -> Result<()> {
        self.request(
            &ClientMessage::Play {
                effect_id: effect.to_string(),
                gain: Some(gain),
            },
            Duration::from_millis(1500),
        )?;
        Ok(())
    }

    async fn stop_all(&self) -> Result<()> {
        self.request(&ClientMessage::StopAll, Duration::from_millis(500))?;
        Ok(())
    }
}

/// Configuracao da acao de parar tudo (nao usa nada, mas o trait exige).
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct NoSettings {
    /// Reservado para uso futuro. Precisa existir para o serde aceitar
    /// structs vazios com `default`.
    _reserved: bool,
}

/// Acao que para todos os efeitos.
struct StopAll;

#[async_trait]
impl Action for StopAll {
    const UUID: &'static str = "com.soundbar.stopall";
    type Settings = NoSettings;

    async fn key_down(
        &self,
        _instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        if let Err(e) = Daemon::new().stop_all().await {
            log::warn!("falha ao parar tudo: {e}");
        }
        Ok(())
    }

    async fn dial_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        self.key_down(instance, settings).await
    }
}

/// Acao principal: toca um efeito.
struct PlayEffect;

#[async_trait]
impl Action for PlayEffect {
    const UUID: &'static str = "com.soundbar.play";
    type Settings = PlaySettings;

    async fn key_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        log::info!(
            "soundbar: keyDown! effect={:?} gain={} toggle={}",
            settings.effect,
            settings.gain,
            settings.toggle
        );
        if settings.effect.trim().is_empty() {
            log::warn!("tecla sem efeito configurado");
            return Ok(());
        }
        let daemon = Daemon::new();
        if settings.toggle {
            daemon.stop_all().await.ok();
        }
        match daemon.play(&settings.effect, settings.gain).await {
            Ok(()) => {
                // Estado visual: a tecla acende enquanto o efeito toca.
                instance.set_state(1).await.ok();
            }
            Err(e) => {
                log::warn!("FALHA ao tocar '{}': {e:#}", settings.effect);
                instance.show_alert().await.ok();
            }
        }
        Ok(())
    }

    async fn key_up(
        &self,
        instance: &Instance,
        _settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        // Volta ao estado ocioso.
        instance.set_state(0).await.ok();
        Ok(())
    }

    async fn dial_down(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        self.key_down(instance, settings).await
    }

    async fn dial_up(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        self.key_up(instance, settings).await
    }

    /// Recebe mensagens do Property Inspector.
    ///
    /// O arquivo chega em base64 dentro do payload, porque o seletor nativo
    /// do navegador (`<input type="file">`) roda dentro do sandbox do
    /// OpenDeck e nao consegue abrir uma janela do sistema por conta propria.
    async fn send_to_plugin(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
        payload: &serde_json::Value,
    ) -> OpenActionResult<()> {
        let Some(data) = payload.get("importData").and_then(|v| v.as_str()) else {
            return Ok(());
        };
        let Some(name) = payload.get("importName").and_then(|v| v.as_str()) else {
            return Ok(());
        };

        match save_imported(data, name) {
            Ok(imported) => {
                log::info!(
                    "soundbar: importado '{}' como id '{}'",
                    imported.name,
                    imported.id
                );
                let mut s = settings.clone();
                s.effect = imported.id.clone();
                instance.set_settings(&s).await.ok();
                remember(instance, &s);
                if let Some(list) = fetch_effects() {
                    let msg = serde_json::json!({ "effects": list, "selected": imported.id });
                    instance.send_to_property_inspector(msg).await.ok();
                }
            }
            Err(e) => {
                log::warn!("soundbar: falha ao importar: {e:#}");
                let msg = serde_json::json!({ "error": format!("{e:#}") });
                instance.send_to_property_inspector(msg).await.ok();
            }
        }
        Ok(())
    }

    /// O host chama isto sempre que salva as settings da tecla.
    ///
    /// E o ponto mais confiavel para manter o backup em dia: em vez de
    /// adivinhar quando o usuario mudou algo, registramos o que o host
    /// gravou de fato.
    async fn did_receive_settings(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        if !settings.effect.trim().is_empty() {
            remember(instance, settings);
        }
        Ok(())
    }

    async fn will_appear(
        &self,
        instance: &Instance,
        settings: &Self::Settings,
    ) -> OpenActionResult<()> {
        log::info!(
            "soundbar: will_appear chamado, effect={:?}",
            settings.effect
        );
        // Recuperacao: se o perfil do OpenDeck perdeu a configuracao desta
        // tecla, o backup tem o valor correto e a gente restaura.
        let mut settings = settings.clone();
        if settings.effect.trim().is_empty() {
            let key = slot_key(instance);
            if let Some(saved) = load_backup().get(&key) {
                if let Some(eff) = saved.get("effect").and_then(|v| v.as_str()) {
                    settings.effect = eff.to_string();
                    if let Some(g) = saved.get("gain").and_then(|v| v.as_f64()) {
                        settings.gain = g as f32;
                    }
                    if let Some(l) = saved.get("label").and_then(|v| v.as_str()) {
                        settings.label = l.to_string();
                    }
                    if let Some(t) = saved.get("toggle").and_then(|v| v.as_bool()) {
                        settings.toggle = t;
                    }
                    instance.set_settings(&settings).await.ok();
                    log::info!("soundbar: restaurou '{}' do backup", settings.effect);
                }
            }
        } else {
            // Configuracao presente: mantem o backup em dia.
            remember(instance, &settings);
        }

        if let Some(list) = fetch_effects() {
            let msg = serde_json::json!({
                "effects": list,
                "selected": settings.effect,
            });
            instance.send_to_property_inspector(msg).await.ok();
        }
        Ok(())
    }
}

/// Efeito recem-importado pelo usuario.
struct ImportedSound {
    id: String,
    name: String,
}

/// Salva um arquivo enviado pelo Property Inspector em base64.
///
/// O navegador entrega o conteudo do arquivo, nao o caminho: dentro do
/// sandbox do OpenDeck nao ha acesso ao filesystem do usuario.
fn save_imported(data_b64: &str, name: &str) -> Result<ImportedSound> {
    use base64::Engine;

    let daemon = Daemon::new();
    let sounds_dir = sounds_dir(&daemon.config_dir)?;

    // Extensao pelo nome original.
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    const OK: &[&str] = &[
        "wav", "mp3", "flac", "ogg", "oga", "opus", "aiff", "m4a", "aac", "wma",
    ];
    if !OK.contains(&ext.as_str()) {
        return Err(anyhow::anyhow!(
            "formato '{ext}' nao suportado. Use: wav, mp3, flac, ogg, opus, aiff, m4a."
        ));
    }

    // O navegador pode mandar data URL (data:audio/mpeg;base64,...).
    let b64 = match data_b64.split_once(",") {
        Some((_, rest)) if data_b64.starts_with("data:") => rest,
        _ => data_b64,
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| anyhow::anyhow!("conteudo invalido: {e}"))?;
    if bytes.is_empty() {
        return Err(anyhow::anyhow!("arquivo vazio"));
    }

    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("efeito")
        .to_string();

    // Nao sobrescreve: o id precisa ser unico para nao quebrar teclas ja feitas.
    let mut dest = sounds_dir.join(format!("{stem}.{ext}"));
    if dest.exists() {
        let mut n = 2;
        loop {
            let cand = sounds_dir.join(format!("{stem}-{n}.{ext}"));
            if !cand.exists() {
                dest = cand;
                break;
            }
            n += 1;
        }
    }

    std::fs::create_dir_all(&sounds_dir)?;
    std::fs::write(&dest, &bytes)
        .map_err(|e| anyhow::anyhow!("falha ao escrever {}: {e}", dest.display()))?;

    let id = dest
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&stem)
        .to_string();

    // Avisa o daemon para recarregar: os sons sao lidos so no startup.
    if let Ok(Some(DaemonMessage::Effects { .. })) =
        daemon.request(&ClientMessage::ReloadEffects, Duration::from_secs(5))
    {
        log::info!("soundbar: daemon recarregou a biblioteca");
    } else {
        log::warn!(
            "soundbar: daemon nao recarregou; reinicie com systemctl --user restart soundbar"
        );
    }

    Ok(ImportedSound { id, name: stem })
}

/// Pasta de sons, a partir da config.
fn sounds_dir(config_dir: &std::path::Path) -> Result<PathBuf> {
    if let Ok(cfg) = Config::load_dir(config_dir) {
        if let Some(d) = cfg.sounds_dir {
            return Ok(d);
        }
    }
    Ok(config_dir.join("sounds"))
}

/// Lista de efeitos disponiveis, direto do disco se o daemon nao responder.
fn fetch_effects() -> Option<Vec<soundbar_core::protocol::EffectInfo>> {
    let d = Daemon::new();
    if let Ok(Some(DaemonMessage::Effects { effects })) =
        d.request(&ClientMessage::ListEffects, Duration::from_millis(1000))
    {
        return Some(effects);
    }
    // fallback: le do disco
    let cfg = Config::load_dir(&d.config_dir).ok()?;
    let dir = cfg
        .sounds_dir
        .unwrap_or_else(|| d.config_dir.join("sounds"));
    let lib = soundbar_core::sfx::load_dir(&dir).ok()?;
    Some(soundbar_core::sfx::infos(&lib))
}

/// Arquivo onde o plugin guarda uma copia das configuracoes de cada tecla.
///
/// O OpenDeck guarda as settings no perfil dele. Se o perfil for recriado,
/// apagado ou corrompido (bug do host, restauracao de backup, troca de
/// dispositivo), tudo se perde. Aqui mantemos uma copia independente, por
/// posicao, e reaplicamos no `will_appear`.
///
/// Chave: "<linha>:<coluna>" -> id do efeito.
fn backup_path() -> PathBuf {
    let dir = Config::resolve_dir();
    dir.join("slots.json")
}

/// Registra a configuracao de uma tecla no backup.
fn remember(instance: &Instance, settings: &PlaySettings) {
    let mut map = load_backup();
    map.insert(
        slot_key(instance),
        serde_json::json!({
            "effect": settings.effect,
            "gain": settings.gain,
            "label": settings.label,
            "toggle": settings.toggle,
        }),
    );
    save_backup(&map);
}

/// Le o backup de slots.
fn load_backup() -> std::collections::BTreeMap<String, serde_json::Value> {
    std::fs::read_to_string(backup_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Grava o backup de slots de forma atomica.
fn save_backup(map: &std::collections::BTreeMap<String, serde_json::Value>) {
    let path = backup_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(s) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(&tmp, s);
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Chave estavel da posicao da tecla.
///
/// Usa as coordenadas quando disponiveis; cai para o id da instancia, que
/// tambem identifica a posicao unicamente.
fn slot_key(instance: &Instance) -> String {
    match instance.coordinates {
        Some(c) => format!("r{}c{}", c.row, c.column),
        None => format!("i{}", instance.instance_id),
    }
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
    simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Stdout,
        simplelog::ColorChoice::Never,
    )
    .ok();

    log::info!("soundbar: iniciando plugin");

    // Handshake para validar versao do protocolo.
    let d = Daemon::new();
    match d.request(
        &ClientMessage::hello("opendeck-plugin", DeviceKind::Unknown),
        Duration::from_millis(800),
    ) {
        Ok(Some(DaemonMessage::Welcome { version, .. })) => {
            log::info!("soundbar: daemon conectado (protocolo v{version})");
        }
        Ok(Some(DaemonMessage::Error { message })) => {
            log::warn!("soundbar: daemon recusou: {message}");
        }
        _ => {
            log::warn!(
                "soundbar: daemon nao encontrado em {} — rode ./install.sh",
                d.endpoint().display()
            );
        }
    }

    register_action(PlayEffect).await;
    register_action(StopAll).await;
    log::info!("soundbar: acao registrada, aguardando teclas");

    run(std::env::args().collect()).await
}
