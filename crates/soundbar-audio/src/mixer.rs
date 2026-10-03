//! Mixagem dos efeitos em um unico buffer de saida.
//!
//! E agnostico de plataforma de proposito: dado um pedido de "tocar este efeito
//! com este ganho", devolve PCM misturado. Testavel sem hardware, o que permite
//! validar a parte que importa (sobreposicao, fade, ganho, polyphony) em CI.

use soundbar_core::sfx::Sound;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Uma voz em reproducao.
#[derive(Debug, Clone)]
pub struct Voice {
    pub instance_id: u64,
    pub effect_id: String,
    /// Indice do proximo sample a ler da fonte.
    cursor: usize,
    pub gain: f32,
    /// Rampa de 0 ate 1 aplicada no inicio, neste numero de samples.
    pub fade_in_len: usize,
    /// Ganho alvo do fade-out (normalmente 0).
    pub fade_out_to: Option<f32>,
    /// Cursor no inicio do fade-out.
    pub fade_start: Option<usize>,
    /// Comprimento do fade-out em samples.
    pub fade_len: usize,
    pub finished: bool,
    /// Quando Some, a voz para ao chegar aqui (modo toggle).
    pub stop_at: Option<u64>,
}

/// Estado do mixer.
pub struct Mixer {
    voices: BTreeMap<u64, Voice>,
    next_id: u64,
    master_gain: f32,
    max_polyphony: usize,
}

impl Default for Mixer {
    fn default() -> Self {
        Mixer::new(1.0, 16)
    }
}

impl Mixer {
    pub fn new(master_gain: f32, max_polyphony: usize) -> Self {
        Mixer { voices: BTreeMap::new(), next_id: 1, master_gain, max_polyphony }
    }

    pub fn set_master_gain(&mut self, g: f32) {
        self.master_gain = g.clamp(0.0, 4.0);
    }

    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    /// Quantas vozes estao ativas.
    pub fn active_voices(&self) -> usize {
        self.voices.values().filter(|v| !v.finished).count()
    }

    /// Instancia de uma voz para `sound`.
    pub fn play(
        &mut self,
        sound: Arc<Sound>,
        gain: f32,
        retrigger: bool,
        stop_at: Option<u64>,
    ) -> Option<u64> {
        // Respeita polyphony: se ja esta no limite e nao ha retrigger, descarta
        // a voz mais antiga para dar lugar a nova.
        if self.max_polyphony > 0 && self.active_voices() >= self.max_polyphony {
            if retrigger {
                if let Some((&oldest, _)) = self.voices.iter().next() {
                    self.voices.remove(&oldest);
                }
            } else {
                return None;
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        self.voices.insert(
            id,
            Voice {
                instance_id: id,
                effect_id: sound.id.clone(),
                cursor: 0,
                gain: gain.clamp(0.0, 4.0),
                fade_in_len: 0,
                fade_out_to: None,
                fade_start: None,
                fade_len: 0,
                finished: false,
                stop_at,
            },
        );
        let _ = sound;
        Some(id)
    }

    /// Aplica fade-out a uma voz.
    pub fn fade_out(&mut self, instance_id: u64, samples: usize, to: f32) -> bool {
        if let Some(v) = self.voices.get_mut(&instance_id) {
            v.fade_in_len = 0;
            v.fade_out_to = Some(to);
            v.fade_start = Some(v.cursor);
            v.fade_len = samples;
            v.stop_at = Some((v.cursor + samples) as u64);
            true
        } else {
            false
        }
    }

    /// Para uma voz imediatamente.
    pub fn stop(&mut self, instance_id: u64) -> bool {
        self.voices.remove(&instance_id).is_some()
    }

    /// Para tudo.
    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

    /// Mistura `frames` frames em `out` (i16 interleaved, 2 canais).
    ///
    /// `lookup` resolve um effect_id para o som, para que o mixer nao precise
    /// conhecer a biblioteca inteira.
    pub fn mix_into(
        &mut self,
        out: &mut [i16],
        lookup: &dyn Fn(&str) -> Option<Arc<Sound>>,
    ) {
        out.iter_mut().for_each(|s| *s = 0);
        let frames = out.len() / 2;
        let mg = self.master_gain as f32;

        for frame in 0..frames {
            let mut acc_l = 0f32;
            let mut acc_r = 0f32;

            for voice in self.voices.values_mut() {
                if voice.finished {
                    continue;
                }
                let Some(sound) = lookup(&voice.effect_id) else {
                    voice.finished = true;
                    continue;
                };
                let total_frames = sound.samples.len() / 2;
                if voice.cursor >= total_frames {
                    voice.finished = true;
                    continue;
                }

                let base = voice.cursor * 2;
                let l = sound.samples[base] as f32;
                let r = sound.samples[base + 1] as f32;

                // envelope: fade-in aplicado, depois fade-out sobreposto.
                let mut env = 1.0f32;
                if voice.fade_in_len > 0 && voice.cursor < voice.fade_in_len {
                    env *= voice.cursor as f32 / voice.fade_in_len as f32;
                }
                if let Some(to) = voice.fade_out_to {
                    // Rampa linear do nivel atual ate `to`
                    // ao longo dos samples do fade.
                    let start = voice.fade_start.unwrap_or(0);
                    let pos = (voice.cursor.saturating_sub(start)) as f32
                        / voice.fade_len.max(1) as f32;
                    env *= 1.0 - (1.0 - to) * pos.min(1.0);
                }

                let g = voice.gain * env * mg;
                acc_l += l * g;
                acc_r += r * g;
                voice.cursor += 1;

                if let Some(end) = voice.stop_at {
                    if voice.cursor as u64 >= end {
                        voice.finished = true;
                    }
                }
            }

            out[frame * 2] = clamp_i16(acc_l);
            out[frame * 2 + 1] = clamp_i16(acc_r);
        }

        // remove vozes terminadas
        self.voices.retain(|_, v| !v.finished);
    }
}

fn clamp_i16(v: f32) -> i16 {
    v.clamp(-32768.0, 32767.0) as i16
}


/// Comandos enviados ao mixer via canal.
#[derive(Debug)]
pub enum MixerCommand {
    Play { sound: std::sync::Arc<soundbar_core::sfx::Sound>, gain: f32, retrigger: bool, stop_at: Option<u64> },
    Stop(u64),
    StopAll,
    SetMasterGain(f32),
    FadeOut { id: u64, samples: usize, to: f32 },
}
