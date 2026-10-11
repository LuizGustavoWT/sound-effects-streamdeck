//! Backend PulseAudio/PipeWire: cria um null-sink virtual e escreve audio mixado nele.
//!
//! O null-sink aparece como dispositivo de saida no sistema. Para ouvir, crie um
//! monitor (`module-remap-source`) ou adicione o sink no mixer do sistema. Para
//! enviar direto a live/gravacao, no OBS use *Captura de Saida de Audio* e
//! selecione este sink.

use anyhow::{anyhow, Result};
use libpulse_binding as pulse;
use pulse::context::{Context, FlagSet as CtxFlags, State as CtxState};
use pulse::def::BufferAttr;
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

/// Envia uma fonte Pulse para um sink, onde ela se soma a outras fontes.
pub fn route_source_to_sink(source: &str, sink: &str) -> Result<()> {
    // Evita duplicar o loopback a cada restart do daemon.
    let existing = std::process::Command::new("pactl")
        .args(["list", "short", "modules"])
        .output()?;
    if existing.status.success() {
        let out = String::from_utf8_lossy(&existing.stdout);
        if out.contains(&format!("source={source}")) && out.contains(&format!("sink={sink}")) {
            return Ok(());
        }
    }

    let status = std::process::Command::new("pactl")
        .args([
            "load-module",
            "module-loopback",
            &format!("source={source}"),
            &format!("sink={sink}"),
            // Latencia curta: voz precisa chegar sem atraso perceptivel.
            "latency_msec=20",
        ])
        .status()?;

    if !status.success() {
        return Err(anyhow!(
            "nao foi possivel rotear a fonte {source} para {sink}"
        ));
    }
    Ok(())
}

/// Remove loopbacks exatos, usado ao migrar uma configuracao antiga.
pub fn remove_loopback(source: &str, sink: &str) -> Result<()> {
    let existing = std::process::Command::new("pactl")
        .args(["list", "short", "modules"])
        .output()?;
    if !existing.status.success() {
        return Ok(());
    }

    let out = String::from_utf8_lossy(&existing.stdout);
    for line in out.lines() {
        let mut fields = line.split_whitespace();
        let Some(id) = fields.next() else { continue };
        let Some(module) = fields.next() else {
            continue;
        };
        let args = fields.collect::<Vec<_>>().join(" ");
        if module == "module-loopback"
            && args.contains(&format!("source={source}"))
            && args.contains(&format!("sink={sink}"))
        {
            let status = std::process::Command::new("pactl")
                .args(["unload-module", id])
                .status()?;
            if !status.success() {
                return Err(anyhow!(
                    "nao foi possivel remover loopback {source} -> {sink}"
                ));
            }
        }
    }
    Ok(())
}

/// Cria uma fonte de audio virtual, para apps que so aceitam microfone.
///
/// Discord, Slack e Google Meet oferecem apenas uma lista de microfones como
/// entrada. Um null-sink (dispositivo de saida) simplesmente nao aparece la.
/// Esta fonte faz o caminho inverso: expõe o audio do sink como se fosse um
/// microfone, entao esses apps passam a enxergar os efeitos.
///
/// `module-remap-source` com `master=<sink>.monitor` e a forma correta no
/// PipeWire: o sink nao tem um "entrada", mas o monitor dele sim.
pub fn ensure_virtual_mic(name: &str, description: &str, master: &str) -> Result<()> {
    let existing = std::process::Command::new("pactl")
        .args(["list", "short", "modules"])
        .output()?;

    if existing.status.success() {
        let out = String::from_utf8_lossy(&existing.stdout);
        for line in out.lines() {
            let mut fields = line.split_whitespace();
            let Some(id) = fields.next() else { continue };
            let Some(module) = fields.next() else {
                continue;
            };
            let args = fields.collect::<Vec<_>>().join(" ");
            if module == "module-remap-source" && args.contains(&format!("source_name={name}")) {
                if args.contains(&format!("master={master}")) {
                    return Ok(());
                }
                let status = std::process::Command::new("pactl")
                    .args(["unload-module", id])
                    .status()?;
                if !status.success() {
                    return Err(anyhow!("nao foi possivel atualizar a fonte virtual {name}"));
                }
                break;
            }
        }
    }

    let status = std::process::Command::new("pactl")
        .args([
            "load-module",
            "module-remap-source",
            &format!("source_name={name}"),
            &format!("master={master}"),
            // Descricao sem espacos: o parser de modulos trunca no espaco.
            &format!(
                "source_properties=device.description={}",
                compact(description)
            ),
        ])
        .status()?;

    if !status.success() {
        return Err(anyhow!(
            "nao foi possivel criar a fonte virtual {name}. O modulo \
             module-remap-source nao esta disponivel?"
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

        // O servidor Pulse aceita por padrao alguns megabytes por stream. Para
        // um gerador de efeitos isso e desnecessario e perigoso: se o clock do
        // null-sink parar, escritas em ritmo fixo viram uma fila que cresce sem
        // limite pratico dentro do pipewire-pulse.
        //
        // Limitamos o stream a 160 ms e pedimos lotes de 20 ms. EARLY_REQUESTS
        // combina com o loop que espera espaco antes de escrever, sem depender
        // de um hardware com relogio proprio (o null-sink nao tem um).
        const SAMPLE_RATE: u32 = 48_000;
        const CHANNELS: u32 = 2;
        const BYTES_PER_FRAME: u32 = CHANNELS * std::mem::size_of::<i16>() as u32;
        const FRAMES_PER_BATCH: usize = SAMPLE_RATE as usize / 50;
        const BYTES_PER_BATCH: usize = FRAMES_PER_BATCH * BYTES_PER_FRAME as usize;
        let buffer_attr = BufferAttr {
            maxlength: (BYTES_PER_BATCH * 8) as u32,
            tlength: (BYTES_PER_BATCH * 4) as u32,
            // Sem prebuffer: um efeito pode comecar imediatamente apos o click.
            prebuf: 0,
            minreq: BYTES_PER_BATCH as u32,
            fragsize: u32::MAX,
        };

        stream
            .connect_playback(
                Some(&self.name),
                Some(&buffer_attr),
                StreamFlags::EARLY_REQUESTS | StreamFlags::START_CORKED,
                None,
                None,
            )
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

        // O stream inicia pausado e so e ativado durante um efeito. Alem de
        // poupar CPU, isso evita alimentar o pipewire-pulse com silencio sem
        // fim. Nunca escrevemos alem do tamanho solicitado pelo servidor.
        let mut corked = true;
        let mut buf: Vec<i16> = Vec::new();
        let mut bytes: Vec<u8> = Vec::with_capacity(BYTES_PER_BATCH);

        while should_run() {
            match ml.iterate(false) {
                IterateResult::Success(_) => {}
                IterateResult::Quit(_) => break,
                IterateResult::Err(e) => {
                    eprintln!("[soundbar] mainloop erro: {e:?}; tentando continuar");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }

            let has_audio = shared
                .mixer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .active_voices()
                > 0;

            if has_audio && corked {
                let _ = stream.uncork(None);
                corked = false;
            } else if !has_audio && !corked {
                let _ = stream.cork(None);
                corked = true;
            }

            if !has_audio {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }

            // A API Pulse informa quantos bytes ela realmente solicitou. Isso
            // e a protecao principal contra uma fila infinita no servidor.
            let writable = stream.writable_size().unwrap_or(0);
            if writable < BYTES_PER_BATCH {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }

            buf.resize(FRAMES_PER_BATCH * CHANNELS as usize, 0);
            {
                let lib = shared
                    .library
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                let mut mx = shared.mixer.lock().unwrap_or_else(|e| e.into_inner());
                mx.mix_into(&mut buf, &move |id: &str| lib.get(id).cloned());
            }

            bytes.resize(buf.len() * std::mem::size_of::<i16>(), 0);
            for (sample, encoded) in buf.iter().zip(bytes.chunks_exact_mut(2)) {
                encoded.copy_from_slice(&sample.to_le_bytes());
            }

            if let Err(e) = stream.write_copy(&bytes, 0, SeekMode::Relative) {
                eprintln!("[soundbar] escrita falhou: {e:?}");
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
        }

        let _ = stream.disconnect();
        Ok(())
    }
}
