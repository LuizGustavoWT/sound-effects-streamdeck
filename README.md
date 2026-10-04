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

Você tem duas opções:

**Opção A: Importar pelo Property Inspector (mais fácil)**
1. No OpenDeck, arraste a ação **Play Effect** para uma tecla.
2. Clique em **Escolher arquivo...** no Property Inspector.
3. Selecione o arquivo de áudio (mp3, wav, ogg, flac) no seu PC.
4. O arquivo é copiado automaticamente para a pasta de sons e fica disponível imediatamente.

**Opção B: Copiar manualmente**
Copie seus arquivos para a pasta de sons:

```bash
cp ~/Downloads/*.wav ~/.config/soundbar-streamdeck/sounds/
```

Formatos aceitos: `.wav` `.mp3` `.flac` `.ogg` `.oga` `.opus` `.aiff` `.m4a` `.aac` `.wma`

Subpastas viram categorias (ex.: `sounds/memes/rickroll.wav` → id `memes/rickroll`).

Depois de copiar manualmente, reinicie o daemon: `systemctl --user restart soundbar`.

### 4. Configurar no OBS

O plugin cria um dispositivo de áudio virtual chamado **StreamDeckSoundBar**. Para levar os efeitos para a live/gravação:

**Passo a passo no OBS:**

1. Abra o OBS e vá em **Fontes** (na parte inferior).
2. Clique no **+** e escolha **Captura de Saída de Áudio** (ou *Audio Output Capture*).
3. Dê um nome (ex: "Efeitos Stream Deck") e clique em OK.
4. Na janela de propriedades, em **Dispositivo**, selecione **StreamDeckSoundBar**.
5. Clique em OK. Pronto! Os efeitos agora entram na sua live.

**Para ouvir os efeitos nos seus fones enquanto transmite:**

Por padrão, o áudio vai só para a live. Se você quiser ouvir também:

- **Linux (PipeWire/WirePlumber):** Abra as configurações de áudio do sistema → Dispositivos de Saída → procure por `StreamDeckSoundBar` → ative o monitor ou redirecione para seus fones.
- **Windows:** Clique com o botão direito no ícone de som → Sons → Gravação → procure por `StreamDeckSoundBar` → Propriedades → Escutar → marque "Escutar este dispositivo" e escolha seus fones.
- **macOS:** Use o **BlackHole** ou **Soundflower** para rotear o áudio, ou configure no mixer do sistema.

> **Importante:** o OBS monta a lista de dispositivos de áudio **uma única
> vez**, quando abre. Se o `soundbar` não estava rodando nesse momento, o
> dispositivo não aparece — e reiniciar o daemon depois não faz o OBS
> relistar. É preciso **fechar e abrir o OBS** para ele enxergar o sink.
>
> O mesmo vale se o daemon for reiniciado: o sink é recriado com um id novo.

**Verificando se está funcionando:**

- Aperte uma tecla com efeito configurado no Stream Deck.
- No OBS, a barra de volume da fonte "Efeitos Stream Deck" deve subir.
- Se não subir, verifique se o daemon está rodando: `systemctl --user status soundbar`

### 5. Testar sem o Stream Deck

Você pode validar o daemon antes de ter o hardware:

```bash
soundbar list                     # efeitos carregados
soundbar play teste               # toca um efeito
soundbar play memes/rickroll -g 0.8
soundbar stop-all
soundbar logs                     # status do daemon
```

### 4b. Usar no Discord, Slack e Google Meet

Esses aplicativos só aceitam **microfones** como entrada de áudio — um
dispositivo de saída virtual (como o `StreamDeckSoundBar`) simplesmente não
aparece na lista deles.

Por isso o daemon cria automaticamente uma **fonte virtual**,
`SoundEffectsStreamDeckMic`, que expõe os efeitos como se fosse um microfone.
Aparecer como microfone é o que esses apps conseguem usar.

1. Rode o `soundbar` normalmente (o daemon precisa estar no ar).
2. No Discord/Slack/Meet, abra as configurações de áudio.
3. Escolha `SoundEffectsStreamDeckMic` como microfone.

### Falar e tocar efeitos ao mesmo tempo

Escolher esse microfone virtual **substitui** o seu microfone real. Para não
ficar trocando de dispositivo em cada call, o daemon pode rotear o seu
microfone de verdade para dentro do sink dos efeitos:

```json
{ "audio": { "mic_into_sink": "alsa_input.usb-SEU-MICROFONE.analog-stereo" } }
```

Ache o nome do seu microfone em `pactl list short sources`. Com isso o sink
passa a receber **voz + efeitos**, e o `StreamDeckSoundBarMic` entrega os dois
num único dispositivo.

Para desligar a fonte virtual (por exemplo, se preferir usar um driver de
áudio dedicado), coloque em `config.json`:

```json
{ "audio": { "virtual_mic": null } }
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
