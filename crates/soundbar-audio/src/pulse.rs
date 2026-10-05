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

/// Retorna a fonte de captura selecionada como microfone padrao no sistema.
pub fn default_source() -> Result<String> {
    let output = std::process::Command::new("pactl")
        .args(["get-default-source"])
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("pactl get-default-source falhou"));
    }

    let source = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if source.is_empty() {
        return Err(anyhow!("pactl retornou uma fonte padrao vazia"));
    }
    Ok(source)
}

/// Envia o microfone para o null-sink; seu monitor leva a mistura ao mic
/// virtual.
pub fn route_mic_into_sink(source: &str, sink: &str) -> Result<()> {
    let modules = std::process::Command::new("pactl")
        .args(["list", "short", "modules"])
        .output()?;
    if modules.status.success() {
        let modules = String::from_utf8_lossy(&modules.stdout);
        if module_loopback_loaded(&modules, source, sink) {
            return Ok(());
        }
        unload_loopbacks_from_sink(&modules, sink)?;
    }

    let mut args = vec!["load-module".to_owned()];
    args.extend(module_loopback_args(source, sink));
    let result = std::process::Command::new("pactl").args(&args).output()?;
    if !result.status.success() {
        return Err(anyhow!(
            "nao foi possivel rotear o microfone {source} para {sink}: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(())
}

fn module_loopback_args(source: &str, sink: &str) -> Vec<String> {
    vec![
        "module-loopback".to_owned(),
        format!("source={source}"),
        format!("sink={sink}"),
        "latency_msec=20".to_owned(),
    ]
}

fn module_loopback_loaded(modules: &str, source: &str, sink: &str) -> bool {
    let source_arg = format!("source={source}");
    let sink_arg = format!("sink={sink}");
    loopback_modules(modules).any(|(_, args)| {
        args.contains(&source_arg.as_str()) && args.contains(&sink_arg.as_str())
    })
}

fn module_loopback_ids_for_sink(modules: &str, sink: &str) -> Vec<String> {
    let sink_arg = format!("sink={sink}");
    loopback_modules(modules)
        .filter_map(|(id, args)| args.contains(&sink_arg.as_str()).then(|| id.to_owned()))
        .collect()
}

fn loopback_modules(modules: &str) -> impl Iterator<Item = (&str, Vec<&str>)> {
    modules.lines().filter_map(|line| {
        let mut columns = line.split_whitespace();
        let id = columns.next()?;
        (columns.next() == Some("module-loopback")).then(|| (id, columns.collect()))
    })
}

fn remove_mic_route_from_sink(sink: &str) -> Result<()> {
    let modules = std::process::Command::new("pactl")
        .args(["list", "short", "modules"])
        .output()?;
    if !modules.status.success() {
        return Err(anyhow!("pactl list short modules falhou"));
    }

    unload_loopbacks_from_sink(&String::from_utf8_lossy(&modules.stdout), sink)
}

fn unload_loopbacks_from_sink(modules: &str, sink: &str) -> Result<()> {
    for id in module_loopback_ids_for_sink(modules, sink) {
        let result = std::process::Command::new("pactl")
            .args(["unload-module", &id])
            .output()?;
        if !result.status.success() {
            return Err(anyhow!("nao foi possivel remover o loopback {id}"));
        }
    }
    Ok(())
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
        .args(["list", "short", "sources"])
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
    pub fn new(name: &str, description: &str, mic_source: Option<String>) -> Result<Self> {
        ensure_null_sink(name, description)?;
        if let Some(source) = mic_source {
            match route_mic_into_sink(&source, name) {
                Ok(()) => eprintln!("[soundbar] microfone roteado: {source} -> {name}"),
                Err(e) => {
                    eprintln!("[soundbar] aviso: nao consegui rotear o microfone {source}: {e:#}")
                }
            }
        } else if let Err(e) = remove_mic_route_from_sink(name) {
            eprintln!("[soundbar] aviso: nao consegui remover a rota do microfone: {e:#}");
        }
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

#[cfg(test)]
mod loopback_tests {
    use super::{module_loopback_args, module_loopback_ids_for_sink, module_loopback_loaded};

    #[test]
    fn loopback_targets_the_requested_mic_and_virtual_sink() {
        assert_eq!(
            module_loopback_args("physical-mic", "StreamDeckSoundBar"),
            vec![
                "module-loopback".to_owned(),
                "source=physical-mic".to_owned(),
                "sink=StreamDeckSoundBar".to_owned(),
                "latency_msec=20".to_owned(),
            ]
        );
    }

    #[test]
    fn existing_loopback_is_detected_only_when_both_endpoints_match() {
        let modules = "12\tmodule-loopback\tsource=physical-mic sink=StreamDeckSoundBar latency_msec=20\n13\tmodule-null-sink\tsink=OtherSink";

        assert!(module_loopback_loaded(
            modules,
            "physical-mic",
            "StreamDeckSoundBar"
        ));
        assert!(!module_loopback_loaded(
            modules,
            "other-mic",
            "StreamDeckSoundBar"
        ));
        assert!(!module_loopback_loaded(
            modules,
            "physical-mic",
            "OtherSink"
        ));
    }

    #[test]
    fn route_cleanup_only_selects_loopbacks_for_the_soundbar_sink() {
        let modules = "12\tmodule-loopback\tsource=physical-mic sink=StreamDeckSoundBar latency_msec=20\n13\tmodule-loopback\tsource=other-mic sink=OtherSink\n14\tmodule-null-sink\tsink=StreamDeckSoundBar";
        assert_eq!(
            module_loopback_ids_for_sink(modules, "StreamDeckSoundBar"),
            vec!["12"]
        );
    }
}
