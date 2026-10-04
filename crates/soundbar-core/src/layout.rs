//! Layout do Stream Deck: mapeia cada chave fisica para um efeito.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Posicao de uma chave. Usa o mesmo sistema do Stream Deck:
/// linha/coluna comecando em 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KeyPosition {
    pub row: usize,
    pub column: usize,
}

impl KeyPosition {
    pub const fn new(row: usize, column: usize) -> Self {
        KeyPosition { row, column }
    }
}

/// Um slot (chave) do deck.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    /// Identificador do efeito a tocar. `None` = slot vazio.
    #[serde(default)]
    pub effect_id: Option<String>,
    /// Ganho por slot (multiplica o master).
    #[serde(default = "one")]
    pub gain: f32,
    /// Faz o slot alternar entre tocar e parar (toggle).
    #[serde(default)]
    pub toggle: bool,
    /// Nao deixa duas copias sobrepor (retrigger).
    #[serde(default = "yes")]
    pub retrigger: bool,
    /// Texto exibido na chave.
    #[serde(default)]
    pub label: Option<String>,
}

fn one() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}

impl Default for Slot {
    fn default() -> Self {
        Slot {
            effect_id: None,
            gain: 1.0,
            toggle: true,
            retrigger: true,
            label: None,
        }
    }
}

/// Layout completo do deck.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Numero de linhas (Stream Deck-classico = 5, XL = 8x5 empilhado).
    pub rows: usize,
    /// Numero de colunas.
    pub columns: usize,
    pub slots: BTreeMap<String, Slot>,
    /// Estado das teclas acorrentadas por `KeyPosition::key()`.
    #[serde(skip)]
    pressed: BTreeMap<String, bool>,
}

impl KeyPosition {
    /// Chave textual estavel usada como indice de mapa.
    pub fn key(&self) -> String {
        format!("{}:{}", self.row, self.column)
    }
}

impl Layout {
    /// Cria um layout de 5x15 (Stream Deck original / Mini / MK.2).
    pub fn standard_15() -> Self {
        Layout {
            rows: 5,
            columns: 15,
            ..Default::default()
        }
    }

    /// Cria um layout de 8 colunas (Stream Deck XL).
    pub fn xl() -> Self {
        Layout {
            rows: 4,
            columns: 8,
            ..Default::default()
        }
    }

    /// Define um slot.
    pub fn set(&mut self, pos: KeyPosition, slot: Slot) {
        self.slots.insert(pos.key(), slot);
    }

    /// Le um slot.
    pub fn get(&self, pos: KeyPosition) -> Option<&Slot> {
        self.slots.get(&pos.key())
    }

    /// Garante que rows/columns >= 1 e que slots fora da grade sejam descartados.
    pub fn sanitize(&mut self) {
        self.rows = self.rows.clamp(1, 16);
        self.columns = self.columns.clamp(1, 32);
        let rows = self.rows;
        let columns = self.columns;
        self.slots.retain(|k, v| {
            v.gain = v.gain.clamp(0.0, 4.0);
            match parse_key(k) {
                Some(p) => p.row < rows && p.column < columns,
                None => false,
            }
        });
    }

    /// Registra estado de tecla pressionada.
    pub fn set_pressed(&mut self, pos: KeyPosition, pressed: bool) {
        self.pressed.insert(pos.key(), pressed);
    }

    /// Consulta estado de tecla pressionada.
    pub fn is_pressed(&self, pos: KeyPosition) -> bool {
        self.pressed.get(&pos.key()).copied().unwrap_or(false)
    }

    /// Zera todos os estados de tecla.
    pub fn release_all(&mut self) {
        self.pressed.clear();
    }
}

/// Converte "row:col" em `KeyPosition`.
pub fn parse_key(s: &str) -> Option<KeyPosition> {
    let (r, c) = s.split_once(':')?;
    Some(KeyPosition::new(r.parse().ok()?, c.parse().ok()?))
}
