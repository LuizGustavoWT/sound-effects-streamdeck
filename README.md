# Sound Effects Stream Deck (OpenDeck)

Transforme seu Stream Deck em uma **soundbar de efeitos** para live e gravação: cada tecla dispara um efeito sonoro no meio da transmissão, e o áudio entra direto na sua live.

Este plugin é feito para o **[OpenDeck](https://github.com/nekename/OpenDeck)** (a alternativa open-source ao software da Elgato no Linux/macOS/Windows). Ele usa a API [OpenAction](https://openaction.amankhanna.me/) nativa do OpenDeck — não precisa de Wine, não precisa do SDK fechado da Elgato.

O plugin cria um **dispositivo de áudio virtual**, então você configura no OBS como qualquer outra fonte — sem plugin de áudio extra, sem driver, sem permissões de root.

```
   ┌──────────────┐   IPC (socket)   ┌──────────┐   áudio   ┌──────────────┐
   │  Plugin .sd  │ ───────────────► │  daemon  │ ────────► │ sink virtual │
   │  (no Stream  │ ◄─────────────── │ + mixer  │           │ p/ OBS       │
   │    Deck)     │    estado/LED    │          │           └──────────────┘
   └──────────────┘                  └──────────┘
```

---

## Instalação (Linux)

Roteiro completo, do zero ao som na live.

### 1. Pré-requisitos

```bash
sudo apt install build-essential pkg-config libasound2-dev pulseaudio-utils
```

> `libasound2-dev` só é necessário se você compilar em uma máquina sem
> PipeWire. Com PipeWire/Pulse installed (padrão no Pop!_OS, Ubuntu 24.04+,
> Fedora), o plugin usa o backend nativo e não precisa de ALSA.

### 2. Instalar o plugin no Stream Deck

```bash
git clone https://github.com/LuizGustavoWT/sound-effects-streamdeck.git
cd sound-effects-streamdeck
./install.sh
```

O script faz tudo sozinho:

1. compila o plugin e o daemon em modo release;
2. cria a pasta `SoundEffectsStreamDeck.sdPlugin`;
3. copia o binário nativo, o `manifest.json` e o instalador;
4. instala o daemon como serviço do usuário (`systemd --user` no Linux,
   `launchd` no macOS, pasta `Startup` no Windows);
5. coloca os efeitos de exemplo.

Quando terminar, o Stream Deck mostra o plugin na lista. Arraste para uma
tecla e configure o efeito no **Property Inspector**.

### 3. Adicionar seus efeitos

Copie seus arquivos para a pasta de sons:

```bash
cp ~/Downloads/*.wav ~/.config/soundbar-streamdeck/sounds/
```

Formatos aceitos: `.wav` `.mp3` `.flac` `.ogg` `.oga` `.opus` `.aiff` `.m4a` `.aac` `.wma`
Subpastas viram categorias (ex.: `sounds/memes/rickroll.wav` → id `memes/rickroll`).

### 4. Configurar no OBS

Como o objetivo é **levar os efeitos para a live/gravação**:

1. No OBS, adicione a fonte **Captura de Saída de Áudio** (ou *Desktop Audio*).
2. Se ela listar vários dispositivos, adicione uma fonte por dispositivo e
   desmarque as que você não quer na live.
3. Confirme que `StreamDeckSoundBar` está na lista e **marcado**.
4. Se a fonte já existir, troque o dispositivo: Properties → dispositivo →
   `StreamDeckSoundBar`.

Se você também quiser **ouvir** os efeitos nos seus fones enquanto transmite,
ative o monitor do dispositivo virtual no seu mixer de áudio
(PipeWire/WirePlumber: *Configurações → Áudio → Dispositivos de Saída →* cliente
`StreamDeckSoundBar`; no Windows, Marque o dispositivo como *Escutar* nas
propriedades; no macOS, use o **Soundflower**/**BlackHole** ou o mixer do
sistema). Sem monitor, o áudio vai só para a live — que é o padrão.

### 5. Testar sem o Stream Deck

Você pode validar o daemon antes de ter o hardware:

```bash
soundbar list                     # efeitos carregados
soundbar play teste               # toca um efeito
soundbar play memes/rickroll -g 0.8
soundbar stop-all
soundbar logs                     # status do daemon
```

---

## Instalação (macOS / Windows)

O mesmo `./install.sh` detecta o sistema. Não é preciso Rust instalado para
**usar** — só para compilar. Quem não quiser compilar baixa o `.sdPlugin`
pronto nos *Releases* do GitHub e arrasta a pasta para
`~/Library/Application Support/StreamDeck/Plugins` (macOS) ou
`%APPDATA%\Elgato\StreamDeck\Plugins` (Windows).

Backend de áudio em outras plataformas: a camada de mixagem é a mesma
(`soundbar-audio`), e a camada de dispositivo é um adapter por SO. **No macOS e
Windows o backend ainda está em desenvolvimento** — o daemon funciona, mas o
dispositivo virtual precisa de um driver de loopback (veja
[Roadmap](#roadmap)).

---

## Como configurar as teclas

No Stream Deck, arraste a ação **Play Effect** para uma tecla. No Property
Inspector você escolhe:

| Campo | O que faz |
|---|---|
| **Effect** | Qual efeito tocar (lista preenchida pelo daemon) |
| **Gain** | Volume deste slot (0–4×) |
| **Toggle** | Primeira tecla liga, segunda desliga (com fade-out) |
| **Label** | Texto exibido na chave |

O ganho global fica em `~/.config/soundbar-streamdeck/config.json`.

---

## Configuração

```json
{
  "audio": {
    "virtual_device": "StreamDeckSoundBar",
    "virtual_device_description": "Sound Effects Stream Deck Output",
    "output": { "kind": "virtual" },
    "master_gain": 1.0,
    "max_polyphony": 16
  },
  "sounds_dir": "~/.config/soundbar-streamdeck/sounds"
}
```

Reinicie o daemon para aplicar: `systemctl --user restart soundbar`.

---

## Roadmap

- [x] Núcleo de áudio (mixer, polyphony, fade, anti-clipping) — testado
- [x] Backend Pulse/PipeWire com null-sink virtual — testado no Linux
- [x] Daemon + IPC (socket Unix) — testado no Linux
- [x] CLI de controle e diagnóstico
- [x] Plugin OpenDeck/OpenAction (manifest `CodePaths` + binário por target-triple) — testado no Linux
- [ ] Backend macOS (CoreAudio / loopback)
- [ ] Backend Windows (WASAPI loopback)
- [ ] Zip multiplataforma na CI

---

## Contribuindo

Contribuições são bem-vindas — issues, PRs e ideias. **Apenas o mantenedor
(LuizGustavoWT) pode fazer merge e fechar issues**, mas todo mundo pode
participar.

Veja [CONTRIBUTING.md](CONTRIBUTING.md).

## Licença

MIT
