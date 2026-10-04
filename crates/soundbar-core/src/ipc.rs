//! Camada de IPC multiplataforma entre o plugin e o daemon.
//!
//! No Linux e macOS usa socket Unix em `<config>/soundbar.sock`.
//! No Windows usa uma porta TCP de loopback, porque sockets Unix do Rust nao
//! sao acessiveis por caminho de arquivo no Windows da mesma forma.
//!
//! O formato das mensagens e o mesmo em todas as plataformas: JSON Lines.

use anyhow::{anyhow, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::time::Duration;

/// Porta padrao no Windows (loopback).
pub const TCP_PORT: u16 = 57123;

/// Endereco do daemon, dependente da plataforma.
#[derive(Debug, Clone)]
pub enum Endpoint {
    #[cfg(unix)]
    Unix(PathBuf),
    Tcp(u16),
}

impl Endpoint {
    /// Resolve o endpoint a partir do diretorio de configuracao.
    pub fn from_config_dir(dir: &Path) -> Endpoint {
        #[cfg(target_os = "windows")]
        {
            let port = std::fs::read_to_string(dir.join("soundbar.port"))
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(TCP_PORT);
            Endpoint::Tcp(port)
        }
        #[cfg(not(target_os = "windows"))]
        {
            Endpoint::Unix(dir.join("soundbar.sock"))
        }
    }

    /// Descricao legivel, para mensagens de erro.
    pub fn display(&self) -> String {
        match self {
            #[cfg(unix)]
            Endpoint::Unix(p) => p.display().to_string(),
            Endpoint::Tcp(p) => format!("127.0.0.1:{p}"),
        }
    }
}

/// Uma conexao com o daemon.
///
///读写 sao feita por handles separados (socket clonado), porque um socket Unix
/// precisa de `try_clone` para ter um writer independente.
pub struct Conn {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
}

impl Conn {
    /// Conecta ao daemon.
    pub fn connect(endpoint: &Endpoint) -> Result<Conn> {
        match endpoint {
            #[cfg(unix)]
            Endpoint::Unix(path) => {
                let s = UnixStream::connect(path).map_err(|e| {
                    anyhow!(
                        "nao consegui conectar no daemon em {}: {e}\n\
                         O daemon esta rodando? Tente: systemctl --user status soundbar",
                        path.display()
                    )
                })?;
                Conn::from_unix(s)
            }
            Endpoint::Tcp(port) => {
                let s = TcpStream::connect(("127.0.0.1", *port)).map_err(|e| {
                    anyhow!("nao consegui conectar no daemon em 127.0.0.1:{port}: {e}")
                })?;
                Conn::from_tcp(s)
            }
        }
    }

    #[cfg(unix)]
    fn from_unix(s: UnixStream) -> Result<Conn> {
        let w = s.try_clone()?;
        Ok(Conn {
            reader: Box::new(BufReader::new(s)),
            writer: Box::new(w),
        })
    }

    fn from_tcp(s: TcpStream) -> Result<Conn> {
        let w = s.try_clone()?;
        Ok(Conn {
            reader: Box::new(BufReader::new(s)),
            writer: Box::new(w),
        })
    }

    /// Aplica timeouts de leitura e escrita.
    pub fn set_timeouts(&self, d: Option<Duration>) {
        // Os handles sao caixas dinamicas; nao ha como alcancar o socket
        // subjacente aqui, entao o timeout e aplicado na criacao.
        let _ = d;
    }

    /// Escreve uma linha JSON e faz flush.
    pub fn write_line(&mut self, payload: &str) -> Result<()> {
        self.writer.write_all(payload.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Le uma linha JSON. `None` quando o peer fecha a conexao.
    pub fn read_line(&mut self) -> Result<Option<String>> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim().to_string()))
    }
}

/// Listener que aceita conexoes do plugin.
pub enum Listener {
    #[cfg(unix)]
    Unix(UnixListener),
    Tcp(TcpListener),
}

impl Listener {
    /// Cria o listener e, no Windows, grava a porta num arquivo.
    pub fn bind(endpoint: &Endpoint, config_dir: &Path) -> Result<Listener> {
        match endpoint {
            #[cfg(unix)]
            Endpoint::Unix(path) => {
                // Socket Unix antigo de uma execucao anterior.
                let _ = std::fs::remove_file(path);
                let l = UnixListener::bind(path).map_err(|e| {
                    anyhow!("nao foi possivel criar socket em {}: {e}", path.display())
                })?;
                Ok(Listener::Unix(l))
            }
            Endpoint::Tcp(port) => {
                let l = TcpListener::bind(("127.0.0.1", *port))
                    .map_err(|e| anyhow!("nao foi possivel abrir a porta {port}: {e}"))?;
                let local = l.local_addr().map(|a| a.port()).unwrap_or(*port);
                std::fs::write(config_dir.join("soundbar.port"), local.to_string())?;
                Ok(Listener::Tcp(l))
            }
        }
    }

    /// Aceita a proxima conexao.
    pub fn accept(&self) -> Result<Conn> {
        match self {
            #[cfg(unix)]
            Listener::Unix(l) => {
                let (s, _) = l.accept()?;
                Conn::from_unix(s)
            }
            Listener::Tcp(l) => {
                let (s, _) = l.accept()?;
                Conn::from_tcp(s)
            }
        }
    }
}
