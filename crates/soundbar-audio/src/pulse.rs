//! Backend PulseAudio/PipeWire: cria um null-sink virtual e escreve audio mixado nele.
//!
//! O null-sink aparece como dispositivo de saida no sistema. Para ouvir, crie um
//! monitor (`module-remap-source`) ou adicione o sink no mixer do sistema. Para
//! enviar direto a live/gravacao, no OBS use *Captura de Saida de Audio* e
//! selecione este sink.

use anyhow::{anyhow, Result};
use libpulse_binding as pulse;
use pulse::context::{Context, FlagSet as CtxFlags, State as CtxState};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::sample::{Format, Spec};
use pulse::stream::{FlagSet as StreamFlags, SeekMode, State as StreamState, Stream};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use crate::mixer::Mixer;
use soundbar_core::sfx::SoundLibrary;

/// Remove espacos da descricao do dispositivo.
///
/// O parser de `load-module` do PulseAudio/PipeWire trunca o valor no
/// primeiro espaco, mesmo com aspas. Uma descricao sem espacos e o que
/// realmente chega ao sistema, e e o que o OBS mostra na lista.
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Cria (ou verifica) o null-sink virtual via pactl.
pub fn ensure_null_sink(name: &str, description: &str) -> Result<()> {
    let existing = std::process::Command::new("pactl")
        .args(["list", "short", "sinks"])
        .output()?;

    if existing.status.success() {
        let out = String::from_utf8_lossy(&existing.stdout);
        if out
            .lines()
            .any(|l| l.split_whitespace().nth(1) == Some(name))
        {
            return Ok(());
        }
    }

    let status = std::process::Command::new("pactl")
        .args([
            "load-module",
            "module-null-sink",
            &format!("sink_name={name}"),
            // O parser de modulos do Pulse corta o valor no primeiro espaco,
            // com ou sem aspas. Por isso a descricao vai sem espacos.
            &format!(
                "sink_properties=device.description={}",
                compact(description)
            ),
            "rate=48000",
            "channels=2",
            "format=s16le",
        ])
        .status()?;

    if !status.success() {
        return Err(anyhow!(
            "pactl load-module module-null-sink falhou. Verifique se o pipewire-pulse esta instalado."
        ));
    }
    Ok(())
}

/// Remove o null-sink (usado no shutdown / --uninstall).
pub fn remove_null_sink(name: &str) {
    let _ = std::process::Command::new("pactl")
        .args([
            "unload-module",
            &format!("module-null-sink sink_name={name}"),
        ])
        .status();
}

/// Estado compartilhado com a thread de audio.
pub struct Shared {
    pub mixer: Arc<Mutex<Mixer>>,
    /// Leitura a cada buffer de audio, escrita apenas no reload.
    /// Por isso RwLock e nao Mutex: nao trava o loop de audio.
    pub library: Arc<RwLock<SoundLibrary>>,
}

/// Backend de saida via PulseAudio.
pub struct PulseOutput {
    name: String,
}

impl PulseOutput {
    /// Prepara o dispositivo. Cria o null-sink se necessario.
    pub fn new(name: &str, description: &str) -> Result<Self> {
        ensure_null_sink(name, description)?;
        Ok(PulseOutput {
            name: name.to_string(),
        })
    }

    pub fn device_name(&self) -> &str {
        &self.name
    }

    /// Roda o laco de audio ate ser interrompido.
    ///
    /// `should_run` e consultado a cada ciclo para permitir shutdown limpo.
    pub fn run(
        &self,
        shared: Shared,
        should_run: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        let spec = Spec {
            format: Format::S16le,
            channels: 2,
            rate: 48_000,
        };

        let mut ml = Mainloop::new().ok_or_else(|| anyhow!("falha ao criar mainloop"))?;
        let mut ctx = Context::new(&ml, "soundbar-daemon")
            .ok_or_else(|| anyhow!("falha ao criar contexto pulse"))?;

        ctx.connect(None, CtxFlags::NOFLAGS, None)
            .map_err(|e| anyhow!("connect ao pulse falhou: {e:?}"))?;

        // Aguarda o contexto ficar pronto.
        let mut waited = Duration::ZERO;
        loop {
            match ml.iterate(false) {
                IterateResult::Success(_) => {}
                IterateResult::Quit(_) => return Err(anyhow!("mainloop encerrou")),
                IterateResult::Err(e) => return Err(anyhow!("mainloop erro: {e:?}")),
            }
            match ctx.get_state() {
                CtxState::Ready => break,
                CtxState::Failed | CtxState::Terminated => {
                    return Err(anyhow!(
                        "nao foi possivel conectar ao servidor Pulse/PipeWire"
                    ))
                }
                _ => {
                    waited += Duration::from_millis(50);
                    if waited > Duration::from_secs(5) {
                        return Err(anyhow!("timeout conectando ao Pulse/PipeWire"));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }

        let mut stream = Stream::new(&mut ctx, "soundbar-out", &spec, None)
            .ok_or_else(|| anyhow!("falha ao criar stream de saida"))?;

        stream
            .connect_playback(Some(&self.name), None, StreamFlags::NOFLAGS, None, None)
            .map_err(|e| anyhow!("connect_playback falhou: {e:?}"))?;

        // `connect_playback` e assincrono: so aceita audio quando o stream
        // chega a Ready. Sem esta espera a primeira escrita falha com -15.
        let mut waited = Duration::ZERO;
        loop {
            match ml.iterate(false) {
                IterateResult::Success(_) => {}
                IterateResult::Quit(_) => return Err(anyhow!("mainloop encerrou")),
                IterateResult::Err(e) => return Err(anyhow!("mainloop erro: {e:?}")),
            }
            match stream.get_state() {
                StreamState::Ready => break,
                StreamState::Failed | StreamState::Terminated => {
                    return Err(anyhow!("stream nao pode abrir em {name}", name = self.name))
                }
                _ => {
                    waited += Duration::from_millis(50);
                    if waited > Duration::from_secs(5) {
                        return Err(anyhow!("timeout abrindo stream de saida"));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }

        let device = self.name.clone();
        eprintln!("[soundbar] tocando em {device}");

        // Laco de escrita. Sem callback: controlamos o ritmo manualmente para
        // poder checar `should_run` e evitar borrow de `stream` dentro de closure.
        // 20 ms por lote: granularidade fina o bastante para nao picotar,
        // grande o bastante para o Pulse nao engasgar.
        const SAMPLE_RATE: u32 = 48_000;
        const FRAMES_PER_BATCH: usize = SAMPLE_RATE as usize / 50;
        let mut buf: Vec<i16> = Vec::new();

        while should_run() {
            match ml.iterate(false) {
                IterateResult::Success(_) => {}
                IterateResult::Quit(_) => break,
                IterateResult::Err(e) => {
                    eprintln!("[soundbar] mainloop erro: {e:?}; tentando continuar");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }

            // `writable_size()` devolve 0 sempre neste sink (e um null-sink
            // sem relogio de consumo), entao o `unwrap_or` nunca era usado e o
            // codigo caia no minimo de 1 frame: 4 bytes por iteracao. Isso
            // produz o audio picotado. Aqui o tamanho vem de um timer fixo.
            let frames = FRAMES_PER_BATCH;

            buf.resize(frames * 2, 0);
            {
                let lib = shared
                    .library
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let mut mx = shared.mixer.lock().unwrap_or_else(|e| e.into_inner());
                mx.mix_into(&mut buf, &move |id: &str| lib.get(id).cloned());
            }

            let bytes: Vec<u8> = buf.iter().flat_map(|s| s.to_le_bytes()).collect();

            if let Err(e) = stream.write_copy(&bytes, 0, SeekMode::Relative) {
                eprintln!("[soundbar] escrita falhou: {e:?}");
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }

            // Espera a duracao real do lote: e o que mantem o ritmo em 48 kHz.
            std::thread::sleep(Duration::from_nanos(
                FRAMES_PER_BATCH as u64 * 1_000_000_000 / SAMPLE_RATE as u64,
            ));
        }

        let _ = stream.disconnect();
        Ok(())
    }
}
