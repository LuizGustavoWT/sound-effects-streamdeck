//! Nucleo compartilhado do Sound Effects Stream Deck.
//!
//! Contiene o modelo de dados (efeitos, slots, layout), a configuracao
//! persistente e o protocolo de mensagens entre o plugin (host do Stream Deck)
//! e o daemon de audio. Nao depende de nenhum sistema de audio, isso permite
//! testar toda a logica em qualquer plataforma.

pub mod config;
pub mod layout;
pub mod protocol;
pub mod sfx;

pub use config::{AudioConfig, Config, ConfigError, OutputDeviceKind};
pub use layout::{KeyPosition, Layout, Slot};
pub use protocol::{
    ClientMessage, DaemonMessage, EffectInfo, HostEvent, HostState, PlaybackStatus, SlotState,
};
