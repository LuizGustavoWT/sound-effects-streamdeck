//! Testes da mixagem: a parte que, se errada, corta ou distorce audio na live.

use soundbar_audio::Mixer;
use soundbar_core::sfx::Sound;
use std::sync::Arc;

/// Cria um som de teste: `frames` frames deSamples constante em `value`.
fn dc(name: &str, frames: usize, value: i16) -> Arc<Sound> {
    let mut s = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        s.push(value);
        s.push(value);
    }
    Arc::new(Sound {
        id: name.to_string(),
        name: name.to_string(),
        sample_rate: 48_000,
        samples: Arc::new(s),
        duration_ms: frames as u64 * 1000 / 48_000,
    })
}

fn lib(sounds: Vec<Arc<Sound>>) -> impl Fn(&str) -> Option<Arc<Sound>> {
    move |id: &str| sounds.iter().find(|s| s.id == id).cloned()
}

fn rms(buf: &[i16]) -> f32 {
    let sum: f64 = buf.iter().map(|&v| (v as f64).powi(2)).sum();
    (sum / buf.len() as f64).sqrt() as f32
}

#[test]
fn silence_when_nothing_plays() {
    let mut m = Mixer::default();
    let mut out = vec![7i16; 64]; // pre-suja: mix_into deve zerar
    m.mix_into(&mut out, &lib(vec![]));
    assert!(out.iter().all(|&v| v == 0));
    assert_eq!(m.active_voices(), 0);
}

#[test]
fn single_voice_passes_signal_through() {
    let s = dc("a", 128, 10_000);
    let mut m = Mixer::new(1.0, 16);
    m.play(s.clone(), 1.0, true, None).unwrap();

    let mut out = vec![0i16; 128 * 2];
    m.mix_into(&mut out, &lib(vec![s]));
    assert_eq!(out[0], 10_000);
    assert!(rms(&out) > 9_000.0);
}

#[test]
fn gain_scales_amplitude() {
    let s = dc("a", 64, 10_000);
    let mut m = Mixer::new(1.0, 16);
    m.play(s.clone(), 0.5, true, None).unwrap();
    let mut out = vec![0i16; 64 * 2];
    m.mix_into(&mut out, &lib(vec![s]));
    assert_eq!(out[0], 5_000);
}

#[test]
fn master_gain_scales_everything() {
    let s = dc("a", 64, 10_000);
    let mut m = Mixer::new(0.25, 16);
    m.play(s.clone(), 1.0, true, None).unwrap();
    let mut out = vec![0i16; 64 * 2];
    m.mix_into(&mut out, &lib(vec![s]));
    assert_eq!(out[0], 2_500);
}

#[test]
fn two_voices_overlap_and_sum() {
    let s = dc("a", 64, 1_000);
    let mut m = Mixer::new(1.0, 16);
    m.play(s.clone(), 1.0, true, None).unwrap();
    m.play(s.clone(), 1.0, true, None).unwrap();
    let mut out = vec![0i16; 64 * 2];
    m.mix_into(&mut out, &lib(vec![s]));
    assert_eq!(out[0], 2_000, "duas vozes devem somar");
    assert_eq!(m.active_voices(), 2);
}

#[test]
fn polyphony_limit_rejects_when_not_retriggering() {
    let s = dc("a", 512, 1_000);
    let mut m = Mixer::new(1.0, 2);
    assert!(m.play(s.clone(), 1.0, true, None).is_some());
    assert!(m.play(s.clone(), 1.0, true, None).is_some());
    assert_eq!(m.active_voices(), 2);
    // terceira voz sem retrigger e descartada
    assert!(m.play(s.clone(), 1.0, false, None).is_none());
    assert_eq!(m.active_voices(), 2);
}

#[test]
fn polyphony_evicts_oldest_when_retriggering() {
    let s = dc("a", 512, 1_000);
    let mut m = Mixer::new(1.0, 2);
    let first = m.play(s.clone(), 1.0, true, None).unwrap();
    m.play(s.clone(), 1.0, true, None).unwrap();
    let third = m.play(s.clone(), 1.0, true, None).unwrap();
    assert_ne!(first, third);
    assert_eq!(m.active_voices(), 2, "a mais antiga deve ter sido evicted");
}

#[test]
fn voice_finishes_after_source_exhausted() {
    let s = dc("a", 32, 5_000);
    let mut m = Mixer::new(1.0, 16);
    m.play(s.clone(), 1.0, true, None).unwrap();

    let mut out = vec![0i16; 64 * 2];
    m.mix_into(&mut out, &lib(vec![s]));
    assert_eq!(m.active_voices(), 0, "voz deve terminar e ser removida");
    // segundo buffer deve sair em silencio
    let mut out2 = vec![0i16; 64 * 2];
    m.mix_into(&mut out2, &lib(vec![]));
    assert!(out2.iter().all(|&v| v == 0));
}

#[test]
fn fade_out_decays_to_silence() {
    let s = dc("a", 4096, 10_000);
    let mut m = Mixer::new(1.0, 16);
    let id = m.play(s.clone(), 1.0, true, None).unwrap();
    assert!(m.fade_out(id, 512, 0.0));

    let mut buf = Vec::new();
    let mut out = vec![0i16; 64 * 2]; // chunks menores para capturar o final do fade
    let lookup = lib(vec![s]);
    for _ in 0..100 {
        m.mix_into(&mut out, &lookup);
        buf.extend_from_slice(&out);
        if m.active_voices() == 0 {
            break;
        }
    }
    // head: inicio em nivel cheio; tail: os ultimos samples, ja no fim do fade
    let head = rms(&buf[..64]);
    let last = rms(&buf[buf.len().saturating_sub(32)..]);
    assert!(head > 8_500.0, "inicio em nivel cheio, rms={head}");
    assert!(
        last < head * 0.05,
        "final deve estar em silencio, head={head} last={last}"
    );
}

#[test]
fn stop_and_stop_all_clear_voices() {
    let s = dc("a", 4096, 1_000);
    let mut m = Mixer::new(1.0, 16);
    let id = m.play(s.clone(), 1.0, true, None).unwrap();
    assert!(m.stop(id));
    assert_eq!(m.active_voices(), 0);

    m.play(s.clone(), 1.0, true, None).unwrap();
    m.play(s.clone(), 1.0, true, None).unwrap();
    assert_eq!(m.active_voices(), 2);
    m.stop_all();
    assert_eq!(m.active_voices(), 0);
}

#[test]
fn output_never_clips_beyond_i16_range() {
    // 10 vozes x 8000 = 80000, muito acima do maximo
    let s = dc("a", 64, 8_000);
    let sounds = vec![s.clone()];
    let mut m = Mixer::new(1.0, 16);
    for _ in 0..10 {
        m.play(s.clone(), 1.0, true, None).unwrap();
    }
    let mut out = vec![0i16; 64 * 2];
    m.mix_into(&mut out, &lib(sounds));
    assert_eq!(out[0], 32_767, "deve saturar em i16::MAX, nao dar overflow");
    assert!(out.iter().all(|&v| v == 32_767));
}

#[test]
fn stop_at_cursor_truncates_voice() {
    let s = dc("a", 4096, 6_000);
    let mut m = Mixer::new(1.0, 16);
    // parar apos 100 samples
    m.play(s.clone(), 1.0, true, Some(100)).unwrap();
    let mut out = vec![0i16; 256 * 2];
    let lookup = lib(vec![s]);
    m.mix_into(&mut out, &lookup);
    assert_eq!(m.active_voices(), 0, "deve parar no cursor configurado");
}
