//! Prova que o carregador de efeitos decodifica arquivos reais.

use soundbar_core::sfx;
use std::path::Path;

/// Escreve um WAV PCM 16 bits estereo com uma senide de 440Hz.
fn write_wav(path: &Path, sample_rate: u32, secs: f32) {
    let frames = (sample_rate as f32 * secs) as u32;
    let n = frames * 2; // 2 canais
    let mut data = Vec::with_capacity(n as usize * 2);
    for i in 0..frames {
        let t = i as f32 / sample_rate as f32;
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 12_000.0;
        for _ in 0..2 {
            data.extend_from_slice(&(v as i16).to_le_bytes());
        }
    }

    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // tamanho do bloco fmt
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&2u16.to_le_bytes()); // canais
    w.extend_from_slice(&sample_rate.to_le_bytes());
    w.extend_from_slice(&(sample_rate * 4).to_le_bytes()); // byte rate
    w.extend_from_slice(&4u16.to_le_bytes()); // block align
    w.extend_from_slice(&16u16.to_le_bytes()); // bits
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    std::fs::write(path, w).unwrap();
}

#[test]
fn decodes_pcm_wav_stereo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("airhorn.wav");
    write_wav(&path, 48_000, 0.25);

    let sound = sfx::load(&path).unwrap();
    assert_eq!(sound.sample_rate, 48_000);
    assert_eq!(sound.samples.len() as u64, 48_000 * 25 / 100 * 2);
    assert!((240..=260).contains(&sound.duration_ms), "duracao foi {}", sound.duration_ms);
    assert_eq!(sound.name, "airhorn");
}

#[test]
fn discovers_and_loads_a_library() {
    let dir = tempfile::tempdir().unwrap();
    let sounds = dir.path().join("sounds");
    let nested = sounds.join("memes");
    std::fs::create_dir_all(&nested).unwrap();
    write_wav(&sounds.join("a.wav"), 44_100, 0.1);
    write_wav(&nested.join("b.wav"), 48_000, 0.1);
    std::fs::write(sounds.join("ignorado.txt"), "nao sou audio").unwrap();

    let found = sfx::discover(&sounds).unwrap();
    assert_eq!(found.len(), 2, "deve ignorar .txt e achar os 2 wavs em subdiretorio");

    let lib = sfx::load_dir(&sounds).unwrap();
    assert_eq!(lib.len(), 2);
    assert!(lib.contains_key("a"), "id deve ser relativo ao diretorio de sons");
    assert!(lib.contains_key("memes/b"), "id aninhado deve preservar o caminho");

    let infos = sfx::infos(&lib);
    assert_eq!(infos.len(), 2);
    assert!(infos.iter().all(|i| i.duration_ms.is_some()));
}

#[test]
fn missing_dir_is_empty_not_error() {
    let lib = sfx::load_dir(Path::new("/nao/existe/nada/aqui")).unwrap();
    assert!(lib.is_empty());
}
