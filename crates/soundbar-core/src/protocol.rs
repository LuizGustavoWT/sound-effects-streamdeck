//! Protocolo de mensagens entre o plugin (que roda dentro do host do Stream Deck)
//! e o daemon que cuida do audio.
//!
//! Transporte: socket unix em Linux/macOS, named pipe TCP em Windows.
//! Framing: JSON Lines (um JSON por linha, terminado em `\n`).

use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

/// Versao do protocolo. Incompatibilidade quebra a conexao de forma limpa.
pub const PROTOCOL_VERSION: u32 = 1;

/// Mensagens que o plugin envia ao daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Handshake inicial.
    Hello { version: u32, host: String, device: DeviceKind },
    /// Pede a lista de efeitos disponiveis.
    ListEffects,
    /// Toca um efeito.
    Play { effect_id: String, gain: Option<f32> },
    /// Para um efeito (por id de instancia).
    Stop { instance_id: u64 },
    /// Para tudo.
    StopAll,
    /// Altera o ganho master.
    SetMasterGain { gain: f32 },
    /// Atualiza o layout completo.
    PushLayout { layout: crate::layout::Layout },
    /// Resposta a um pedido de status.
    Ping,
}

impl ClientMessage {
    pub fn hello(host: impl Into<String>, device: DeviceKind) -> Self {
        ClientMessage::Hello { version: PROTOCOL_VERSION, host: host.into(), device }
    }
}

/// Tipo de hardware conectado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Original,
    Mini,
    Mk2,
    Xl,
    Plus,
    Unknown,
}

impl DeviceKind {
    /// Layout sugerido para este hardware.
    pub fn default_grid(self) -> (usize, usize) {
        match self {
            DeviceKind::Xl => (4, 8),
            DeviceKind::Plus => (1, 3),
            DeviceKind::Mini | DeviceKind::Mk2 => (3, 5),
            _ => (5, 15),
        }
    }
}

/// Mensagens que o daemon envia ao plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonMessage {
    /// Handshake aceito.
    Welcome { version: u32, device: Option<String> },
    /// Lista de efeitos.
    Effects { effects: Vec<EffectInfo> },
    /// Confirma que comecou a tocar.
    Playing { instance_id: u64, effect_id: String },
    /// Confirmacao de parada.
    Stopped { instance_id: u64 },
    /// Estado de uma tecla (para acender/desacender no LED).
    SlotState { key: String, state: SlotState },
    /// Resposta a ping.
    Pong { host: String, uptime_ms: u64 },
    /// Erro nao fatal.
    Error { message: String },
}

/// Estado de uma tecla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotState {
    Idle,
    Playing,
    Stopped,
}

/// Estado de uma reproducao.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Started,
    Retriggered,
    Rejected,
}

/// Informacao de um efeito disponivel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectInfo {
    pub id: String,
    pub name: String,
    /// Duracao em ms, se conhecida.
    pub duration_ms: Option<u64>,
}

/// Evento vindo do host do Stream Deck, ja normalizado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// Tecla pressionada na posicao dada.
    KeyDown { row: usize, column: usize },
    /// Tecla solta.
    KeyUp { row: usize, column: usize },
    /// Mudanca de layout do host.
    LayoutChanged { rows: usize, columns: usize },
    /// Plugin foi fechado.
    Shutdown,
}

/// Estado reportado pelo host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostState {
    pub connected: bool,
    pub rows: usize,
    pub columns: usize,
}

/// Le uma mensagem JSON de um reader com framing de linha.
pub fn read_message<R: BufRead>(r: &mut R) -> anyhow::Result<Option<String>> {
    let mut line = String::new();
    let n = r.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(line))
}

/// Escreve uma mensagem JSON seguida de `\n`.
pub fn write_message<W: Write>(w: &mut W, payload: &impl Serialize) -> anyhow::Result<()> {
    let s = serde_json::to_string(payload)?;
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_message_roundtrip() {
        let m = ClientMessage::Play { effect_id: "airhorn".into(), gain: Some(0.5) };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"type\":\"play\""));
        let back: ClientMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, ClientMessage::Play { ref effect_id, .. } if effect_id == "airhorn"));
    }

    #[test]
    fn daemon_message_roundtrip() {
        let m = DaemonMessage::SlotState { key: "0:0".into(), state: SlotState::Playing };
        let s = serde_json::to_string(&m).unwrap();
        let back: DaemonMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, DaemonMessage::SlotState { .. }));
    }

    #[test]
    fn device_default_grids() {
        assert_eq!(DeviceKind::Original.default_grid(), (5, 15));
        assert_eq!(DeviceKind::Xl.default_grid(), (4, 8));
        assert_eq!(DeviceKind::Plus.default_grid(), (1, 3));
    }
}
