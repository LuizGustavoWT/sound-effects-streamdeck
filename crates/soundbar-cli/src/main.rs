//! CLI de controle e diagnostico do soundbar.
//!
//! Conversa com o daemon pelo mesmo socket IPC do plugin do Stream Deck,
//! para que voce possa testar efeitos sem o hardware na mesa.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use soundbar_core::config::Config;
use soundbar_core::ipc::{Conn, Endpoint};
use soundbar_core::protocol::{ClientMessage, DaemonMessage};

const HELP: &str = "soundbar - controle do Sound Effects Stream Deck

USO:
    soundbar list                    lista os efeitos carregados
    soundbar play <id> [-g <ganho>]  toca um efeito
    soundbar stop-all                para tudo
    soundbar gain <valor>            define o ganho master (0.0 - 4.0)
    soundbar status                  mostra o estado do daemon
    soundbar config                  mostra os caminhos usados
    soundbar --help                  esta ajuda

EXEMPLOS:
    soundbar list
    soundbar play teste
    soundbar play memes/rickroll -g 0.8
    soundbar stop-all
";

fn main() {
    if let Err(e) = real_main() {
        eprintln!("erro: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // --config <dir>
    let mut config_dir = Config::resolve_dir();
    let rest: Vec<String> = match args.iter().position(|a| a == "--config") {
        Some(i) => {
            config_dir = PathBuf::from(
                args.get(i + 1)
                    .ok_or_else(|| anyhow!("--config precisa de um caminho"))?,
            );
            let mut v = args.clone();
            v.remove(i);
            v.remove(i);
            v
        }
        None => args.clone(),
    };

    let cmd = rest.first().map(|s| s.as_str());

    match cmd {
        None | Some("--help") | Some("-h") | Some("help") => {
            print!("{HELP}");
            Ok(())
        }
        Some("list") => cmd_list(&config_dir),
        Some("play") => cmd_play(&rest[1..], &config_dir),
        Some("stop-all") => {
            send(&config_dir, &ClientMessage::StopAll)?;
            println!("parado");
            Ok(())
        }
        Some("gain") => {
            let v: f32 = rest
                .get(1)
                .ok_or_else(|| anyhow!("uso: soundbar gain <valor>"))?
                .parse()
                .context("ganho deve ser um numero")?;
            send(&config_dir, &ClientMessage::SetMasterGain { gain: v })?;
            println!("ganho master = {v}");
            Ok(())
        }
        Some("status") => cmd_status(&config_dir),
        Some("config") => cmd_config(&config_dir),
        Some(other) => Err(anyhow!("comando desconhecido: {other}\n\n{HELP}")),
    }
}

/// Envia uma mensagem e devolve a resposta (se houver).
fn send(config_dir: &std::path::Path, msg: &ClientMessage) -> Result<Option<DaemonMessage>> {
    let endpoint = Endpoint::from_config_dir(config_dir);
    let mut conn = Conn::connect(&endpoint)?;
    conn.write_line(&serde_json::to_string(msg)?)?;
    let Some(line) = conn.read_line()? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str::<DaemonMessage>(&line)?))
}

fn cmd_list(config_dir: &std::path::Path) -> Result<()> {
    // Tenta o daemon; se nao estiver de pe, le os sons direto do disco.
    if let Ok(Some(DaemonMessage::Effects { effects })) =
        send(config_dir, &ClientMessage::ListEffects)
    {
        if effects.is_empty() {
            println!("(nenhum efeito carregado)");
            return Ok(());
        }
        for e in effects {
            let dur = e
                .duration_ms
                .map(|d| format!("{d}ms"))
                .unwrap_or_else(|| "?".into());
            println!("{:<32} {:>8}  {}", e.id, dur, e.name);
        }
        return Ok(());
    }

    eprintln!("(daemon fora do ar; lendo os sons direto do disco)\n");
    let cfg = Config::load_dir(config_dir)?;
    let dir = cfg.sounds_dir.unwrap_or_else(|| config_dir.join("sounds"));
    let lib = soundbar_core::sfx::load_dir(&dir)?;
    if lib.is_empty() {
        println!("(nenhum efeito em {})", dir.display());
        return Ok(());
    }
    for (id, s) in &lib {
        println!("{:<32} {:>8}ms  {}", id, s.duration_ms, s.name);
    }
    Ok(())
}

fn cmd_play(args: &[String], config_dir: &std::path::Path) -> Result<()> {
    let mut effect_id = None;
    let mut gain = 1.0f32;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-g" | "--gain" => {
                i += 1;
                gain = args
                    .get(i)
                    .ok_or_else(|| anyhow!("-g precisa de um valor"))?
                    .parse()
                    .context("ganho deve ser um numero")?;
            }
            other => {
                if effect_id.is_some() {
                    return Err(anyhow!("uso: soundbar play <id> [-g <ganho>]"));
                }
                effect_id = Some(other.to_string());
            }
        }
        i += 1;
    }
    let id = effect_id.ok_or_else(|| anyhow!("uso: soundbar play <id> [-g <ganho>]"))?;

    match send(
        config_dir,
        &ClientMessage::Play {
            effect_id: id.clone(),
            gain: Some(gain),
        },
    )? {
        Some(DaemonMessage::Playing {
            instance_id,
            effect_id,
        }) => {
            println!("tocando '{effect_id}' (instancia {instance_id}, ganho {gain})");
            Ok(())
        }
        Some(DaemonMessage::Error { message }) => Err(anyhow!("daemon: {message}")),
        _ => Err(anyhow!("sem resposta do daemon")),
    }
}

fn cmd_status(config_dir: &std::path::Path) -> Result<()> {
    match send(
        config_dir,
        &ClientMessage::hello("cli", soundbar_core::protocol::DeviceKind::Unknown),
    )? {
        Some(DaemonMessage::Welcome { version, device }) => {
            println!("daemon: rodando (protocolo v{version})");
            let _ = device;
        }
        Some(DaemonMessage::Error { message }) => {
            println!("daemon: respondeu com erro: {message}")
        }
        _ => println!("daemon: sem resposta"),
    }
    if let Some(DaemonMessage::Pong { host, uptime_ms }) = send(config_dir, &ClientMessage::Ping)? {
        println!("{host} — no ar ha {}s", uptime_ms / 1000);
    }
    Ok(())
}

fn cmd_config(config_dir: &std::path::Path) -> Result<()> {
    println!("config:  {}", config_dir.display());
    println!(
        "socket:  {}",
        Endpoint::from_config_dir(config_dir).display()
    );
    match Config::load_dir(config_dir) {
        Ok(cfg) => println!(
            "sons:    {}",
            cfg.sounds_dir.clone().unwrap_or_default().display()
        ),
        Err(e) => println!("sons:    (erro ao ler config: {e})"),
    }
    Ok(())
}
