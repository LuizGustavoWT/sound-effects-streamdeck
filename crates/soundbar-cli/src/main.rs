//! CLI de controle e diagnostico do soundbar.
//!
//! Conversa com o daemon pelo mesmo socket IPC do plugin do Stream Deck,
//! para que voce possa testar efeitos sem o hardware na mesa.

use anyhow::{anyhow, Context, Result};
use soundbar_core::config::Config;
use soundbar_core::protocol::{ClientMessage, DaemonMessage};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

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

fn socket_path(config_dir: &std::path::Path) -> PathBuf {
    config_dir.join("soundbar.sock")
}

fn connect(config_dir: &std::path::Path) -> Result<UnixStream> {
    let p = socket_path(config_dir);
    UnixStream::connect(&p).map_err(|e| {
        anyhow!(
            "nao consegui conectar no daemon em {}: {e}\n\
             O daemon esta rodando? Tente: systemctl --user status soundbar",
            p.display()
        )
    })
}

/// Envia uma mensagem e imprime a resposta (se houver).
fn send(config_dir: &std::path::Path, msg: &ClientMessage) -> Result<Option<DaemonMessage>> {
    let mut s = connect(config_dir)?;
    let payload = serde_json::to_string(msg)?;
    s.write_all(payload.as_bytes())?;
    s.write_all(b"\n")?;
    s.flush()?;

    let mut reader = BufReader::new(&s);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let reply = serde_json::from_str::<DaemonMessage>(line.trim())?;
    Ok(Some(reply))
}

fn cmd_list(config_dir: &std::path::Path) -> Result<()> {
    // Tenta o daemon; se nao estiver de pe, le os sons direto do disco.
    if let Ok(Some(DaemonMessage::Effects { effects })) = send(config_dir, &ClientMessage::ListEffects)
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

    match send(config_dir, &ClientMessage::Play { effect_id: id.clone(), gain: Some(gain) })? {
        Some(DaemonMessage::Playing { instance_id, effect_id }) => {
            println!("tocando '{effect_id}' (instancia {instance_id}, ganho {gain})");
            Ok(())
        }
        Some(DaemonMessage::Error { message }) => Err(anyhow!("daemon: {message}")),
        _ => Err(anyhow!("sem resposta do daemon")),
    }
}

fn cmd_status(config_dir: &std::path::Path) -> Result<()> {
    match send(config_dir, &ClientMessage::hello("cli", soundbar_core::protocol::DeviceKind::Unknown))? {
        Some(DaemonMessage::Welcome { version, device }) => {
            println!("daemon: rodando (protocolo v{version})");
            let _ = device;
        }
        Some(DaemonMessage::Error { message }) => {
            println!("daemon: respondeu com erro: {message}")
        }
        _ => println!("daemon: sem resposta"),
    }
    match send(config_dir, &ClientMessage::Ping)? {
        Some(DaemonMessage::Pong { host, uptime_ms }) => {
            println!("{host} — no ar ha {}s", uptime_ms / 1000);
        }
        _ => {}
    }
    Ok(())
}

fn cmd_config(config_dir: &std::path::Path) -> Result<()> {
    println!("config:  {}", config_dir.display());
    println!("socket:  {}", socket_path(config_dir).display());
    match Config::load_dir(config_dir) {
        Ok(cfg) => println!("sons:    {}", cfg.sounds_dir.clone().unwrap_or_default().display()),
        Err(e) => println!("sons:    (erro ao ler config: {e})"),
    }
    Ok(())
}
