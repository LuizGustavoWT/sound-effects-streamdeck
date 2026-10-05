//! Carregamento e decodificacao de arquivos de audio.
//!
//! Usa Symphonia para decodificar uma vez em PCM e guardar em memoria. Os
//! efeitos de stream sao curtos, entao manter em memoria evita latencia de disco
//! no momento da tecla.

use crate::protocol::EffectInfo;
use anyhow::{anyhow, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Audio decodificado e pronto para mixagem.
#[derive(Debug, Clone)]
pub struct Sound {
    pub id: String,
    pub name: String,
    pub sample_rate: u32,
    /// Samples interleaved, 2 canais (i16).
    pub samples: Arc<Vec<i16>>,
    pub duration_ms: u64,
}

/// Um efeito carregado.
pub type SoundLibrary = BTreeMap<String, Arc<Sound>>;

/// Extensoes de arquivo suportadas.
const SUPPORTED: &[&str] = &[
    "wav", "mp3", "flac", "ogg", "oga", "opus", "aiff", "aif", "aifc", "m4a", "aac", "wma",
];

/// Descobre arquivos de audio em um diretorio, recursivamente.
pub fn discover(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in
            std::fs::read_dir(&d).with_context(|| format!("lendo diretorio {}", d.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| SUPPORTED.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Gera um id estavel a partir do caminho relativo.
pub fn id_for(dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(dir).unwrap_or(path);
    rel.with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Le e decodifica um unico arquivo, convertendo para 2 canais i16.
pub fn load(path: &Path) -> Result<Sound> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).with_context(|| format!("abrindo {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let hint = Hint::new();
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("sondando formato de {}", path.display()))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("{}: nenhuma faixa de audio", path.display()))?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| format!("criando decoder para {}", path.display()))?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
    let mut interleaved: Vec<i16> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(e).context(format!("lendo pacote de {}", path.display())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        sample_rate = spec.rate;
        let mut sb = SampleBuffer::<i16>::new(decoded.capacity() as u64, spec);
        sb.copy_interleaved_ref(decoded);
        interleaved.extend_from_slice(sb.samples());
    }

    let frames = interleaved.len() / 2;
    let duration_ms = if sample_rate > 0 {
        frames as u64 * 1000 / sample_rate as u64
    } else {
        0
    };
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    Ok(Sound {
        id: name.clone(),
        name,
        sample_rate,
        samples: Arc::new(interleaved),
        duration_ms,
    })
}

/// Carrega todos os efeitos de um diretorio.
pub fn load_dir(dir: &Path) -> Result<SoundLibrary> {
    let mut lib = BTreeMap::new();
    for path in discover(dir)? {
        match load(&path) {
            Ok(mut sound) => {
                let id = id_for(dir, &path);
                // The daemon uses Sound.id to stop the current voice before
                // replaying an effect. Keep it identical to the public
                // library key, including any subdirectory prefix.
                sound.id = id.clone();
                lib.insert(id, Arc::new(sound));
            }
            Err(e) => {
                eprintln!("[soundbar] pulando {}: {e:#}", path.display());
            }
        }
    }
    Ok(lib)
}

/// Converte a biblioteca em `EffectInfo` para o protocolo.
pub fn infos(lib: &SoundLibrary) -> Vec<EffectInfo> {
    lib.values()
        .map(|s| EffectInfo {
            id: s.id.clone(),
            name: s.name.clone(),
            duration_ms: Some(s.duration_ms),
        })
        .collect()
}
