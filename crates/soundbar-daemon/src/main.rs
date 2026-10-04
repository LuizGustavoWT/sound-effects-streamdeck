//! Daemon do Sound Effects Stream Deck.
//!
//! Fica rodando em background e faz duas coisas ao mesmo tempo:
//!   1. Escreve o audio mixado no sink virtual (loop de audio).
//!   2. Escuta um socket IPC onde o plugin do Stream Deck manda "tocar isso".
//!
//! O plugin nunca fala com o audio direto: ele so conversa com este daemon.

use anyhow::{anyhow, Context, Result};
use soundbar_audio::mixer::Mixer;
use soundbar_core::config::Config;
use soundbar_core::ipc::{Conn, Endpoint, Listener};
use soundbar_core::protocol::{ClientMessage, DaemonMessage};
use soundbar_core::sfx::{self, SoundLibrary};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Estado compartilhado entre o loop de audio e as conexoes IPC.
struct AppState {
    mixer: Arc<Mutex<Mixer>>,
    library: Arc<SoundLibrary>,
    started: Instant,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("run");

    match cmd {
        "run" | "" => run(&args),
        "--help" | "-h" | "help" => {
            println!("{}", HELP);
            Ok(())
        }
        "--version" | "-V" | "version" => {
            println!("soundbar-daemon {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(anyhow!("comando desconhecido: {other}\n\n{HELP}")),
    }
}

const HELP: &str = "soundbar-daemon - daemon de audio do Sound Effects Stream Deck

USO:
    soundbar-daemon run              roda o daemon (padrao)
    soundbar-daemon --help           mostra esta ajuda
    soundbar-daemon --version        mostra a versao

O daemon cria o dispositivo virtual 'StreamDeckSoundBar' e escreve nele
o audio dos efeitos. No OBS, adicione uma fonte de Audio de Saida e
escolha esse dispositivo para levar os efeitos a live/gravacao.";

fn run(args: &[String]) -> Result<()> {
    // --config <dir> opcional
    let mut config_dir = Config::resolve_dir();
    if let Some(i) = args.iter().position(|a| a == "--config") {
        let v = args
            .get(i + 1)
            .ok_or_else(|| anyhow!("--config precisa de um caminho"))?;
        config_dir = PathBuf::from(v);
    }

    let mut cfg = Config::load_dir(&config_dir)?;
    cfg.sanitize();
    std::fs::create_dir_all(&config_dir).ok();

    let sounds_dir = cfg
        .sounds_dir
        .clone()
        .unwrap_or_else(|| config_dir.join("sounds"));

    eprintln!("[soundbar] configuracao: {}", config_dir.display());
    eprintln!("[soundbar] sons: {}", sounds_dir.display());

    let library = Arc::new(load_library(&sounds_dir)?);
    if library.is_empty() {
        eprintln!(
            "[soundbar] AVISO: nenhum efeito encontrado em {}. \
             Coloque .wav/.mp3/.ogg ali (veja README).",
            sounds_dir.display()
        );
    } else {
        eprintln!("[soundbar] {} efeitos carregados", library.len());
    }

    let state = Arc::new(AppState {
        mixer: Arc::new(Mutex::new(Mixer::new(cfg.audio.master_gain, cfg.audio.max_polyphony))),
        library,
        started: Instant::now(),
    });

    // IPC multiplataforma: socket Unix no Linux/macOS, loopback TCP no Windows.
    let endpoint = Endpoint::from_config_dir(&config_dir);
    let listener = Listener::bind(&endpoint, &config_dir)?;
    eprintln!("[soundbar] IPC em {}", endpoint.display());

    let running = Arc::new(AtomicBool::new(true));

    // Thread de audio (Linux/Pulse)
    #[cfg(target_os = "linux")]
    {
        use soundbar_audio::pulse::{PulseOutput, Shared};
        let out = PulseOutput::new(&cfg.audio.virtual_device, &cfg.audio.virtual_device_description)
            .context("falha ao preparar dispositivo de saida")?;

        let run_flag = running.clone();
        let audio_state = state.clone();
        let should = Arc::new(move || run_flag.load(Ordering::Relaxed));
        std::thread::spawn(move || {
            // Compartilha o mesmo mutex do AppState, para que os comandos IPC
            //afetam exatamente o que o loop de audio esta escrevendo.
            let shared = Shared { mixer: audio_state.mixer.clone(), library: audio_state.library.clone() };
            if let Err(e) = out.run(shared, should) {
                eprintln!("[soundbar] loop de audio encerrou: {e:#}");
            }
        });
    }

    // Loop de IPC
    eprintln!("[soundbar] pronto. Ctrl-C para sair.");
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok(conn) => {
                let st = state.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_client(conn, st) {
                        eprintln!("[soundbar] cliente desconectou: {e:#}");
                    }
                });
            }
            Err(e) => {
                eprintln!("[soundbar] erro aceitando conexao: {e}");
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    Ok(())
}

fn load_library(dir: &std::path::Path) -> Result<SoundLibrary> {
    let lib = sfx::load_dir(dir)?;
    for (id, s) in &lib {
        eprintln!("[soundbar]   efeito '{id}' ({:?}, {}ms)", s.sample_rate, s.duration_ms);
    }
    Ok(lib)
}

fn handle_client(mut conn: Conn, state: Arc<AppState>) -> Result<()> {
    loop {
        let Some(line) = conn.read_line()? else {
            break; // cliente fechou
        };
        if line.is_empty() {
            continue;
        }
        let msg: ClientMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                let err = DaemonMessage::Error { message: format!("JSON invalido: {e}") };
                conn.write_line(&serde_json::to_string(&err)?)?;
                continue;
            }
        };

        let reply = dispatch(msg, &state);
        if let Some(r) = reply {
            conn.write_line(&serde_json::to_string(&r)?)?;
        }
    }
    Ok(())
}

fn dispatch(msg: ClientMessage, state: &Arc<AppState>) -> Option<DaemonMessage> {
    match msg {
        ClientMessage::Hello { version, host, device } => {
            if version != soundbar_core::protocol::PROTOCOL_VERSION {
                return Some(DaemonMessage::Error {
                    message: format!(
                        "versao de protocolo incompativel: plugin={version} daemon={}",
                        soundbar_core::protocol::PROTOCOL_VERSION
                    ),
                });
            }
            let _ = device;
            Some(DaemonMessage::Welcome {
                version: soundbar_core::protocol::PROTOCOL_VERSION,
                device: Some(host),
            })
        }

        ClientMessage::ListEffects => Some(DaemonMessage::Effects {
            effects: sfx::infos(&state.library),
        }),

        ClientMessage::Play { effect_id, gain } => {
            let Some(sound) = state.library.get(&effect_id).cloned() else {
                return Some(DaemonMessage::Error {
                    message: format!("efeito desconhecido: {effect_id}"),
                });
            };
            let g = gain.unwrap_or(1.0);
            let id = {
                let mut mx = state.mixer.lock().unwrap_or_else(|e| e.into_inner());
                mx.play(sound, g, true, None)
            };
            match id {
                Some(i) => Some(DaemonMessage::Playing { instance_id: i, effect_id }),
                None => Some(DaemonMessage::Error {
                    message: "polyphony lotada".into(),
                }),
            }
        }

        ClientMessage::Stop { instance_id } => {
            let ok = {
                let mut mx = state.mixer.lock().unwrap_or_else(|e| e.into_inner());
                mx.stop(instance_id)
            };
            if ok {
                Some(DaemonMessage::Stopped { instance_id })
            } else {
                Some(DaemonMessage::Error { message: format!("instancia {instance_id} nao existe") })
            }
        }

        ClientMessage::StopAll => {
            let mut mx = state.mixer.lock().unwrap_or_else(|e| e.into_inner());
            mx.stop_all();
            None
        }

        ClientMessage::SetMasterGain { gain } => {
            let mut mx = state.mixer.lock().unwrap_or_else(|e| e.into_inner());
            mx.set_master_gain(gain);
            None
        }

        ClientMessage::PushLayout { .. } => None,

        ClientMessage::Ping => Some(DaemonMessage::Pong {
            host: format!("soundbar-daemon {}", env!("CARGO_PKG_VERSION")),
            uptime_ms: state.started.elapsed().as_millis() as u64,
        }),
    }
}
