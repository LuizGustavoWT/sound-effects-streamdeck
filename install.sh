#!/usr/bin/env bash
# Instalador do Sound Effects Stream Deck.
#
#   ./install.sh              compila, instala e registra o daemon
#   ./install.sh --uninstall  remove tudo
#   ./install.sh --rebuild    recompila sem reinstalar o daemon
#   ./install.sh --dev        instala em modo debug
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_NAME="SoundEffectsStreamDeck"
PROFILE="release"
ACTION="install"

for arg in "$@"; do
  case "$arg" in
    --uninstall) ACTION="uninstall" ;;
    --rebuild)   ACTION="rebuild" ;;
    --dev)       PROFILE="debug" ;;
    -h|--help)   sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "opcao desconhecida: $arg" >&2; exit 2 ;;
  esac
done

say()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[!]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }

os() { uname -s; }

# --- diretorios ----------------------------------------------------------
config_dir() {
  case "$(os)" in
    Linux)   echo "${XDG_CONFIG_HOME:-$HOME/.config}/soundbar-streamdeck" ;;
    Darwin)  echo "$HOME/Library/Application Support/soundbar-streamdeck" ;;
    *)       echo "${APPDATA:-$HOME/.config}/soundbar-streamdeck" ;;
  esac
}

plugin_dir() {
  case "$(os)" in
    Linux)
      # Stream Deck 5+ em formato .sdPlugin
      for d in \
        "${XDG_CONFIG_HOME:-$HOME/.config}/StreamDeck/plugins" \
        "$HOME/.config/streamdeck/plugins" \
        "$HOME/.local/share/StreamDeck/plugins" \
        "$HOME/.config/Elgato/StreamDeck/plugins"
      do
        [ -d "$d" ] && { echo "$d"; return; }
      done
      echo "$HOME/.config/streamdeck/plugins"
      ;;
    Darwin) echo "$HOME/Library/Application Support/StreamDeck/Plugins" ;;
    *)      echo "${APPDATA:-$HOME/AppData/Roaming}/Elgato/StreamDeck/Plugins" ;;
  esac
}

# --- uninstall -----------------------------------------------------------
if [ "$ACTION" = "uninstall" ]; then
  say "Removendo $PLUGIN_NAME"
  CFG="$(config_dir)"; PLG="$(plugin_dir)"

  case "$(os)" in
    Linux)   systemctl --user stop soundbar 2>/dev/null || true
             systemctl --user disable soundbar 2>/dev/null || true
             rm -f "$HOME/.config/systemd/user/soundbar.service" ;;
    Darwin)  launchctl bootout "gui/$(id -u)" "$HOME/Library/LaunchAgents/dev.soundbar.plist" 2>/dev/null || true
             rm -f "$HOME/Library/LaunchAgents/dev.soundbar.plist" ;;
  esac

  pactl unload-module "module-null-sink sink_name=StreamDeckSoundBar" 2>/dev/null || true
  rm -rf "$PLG/$PLUGIN_NAME.sdPlugin"
  say "Plugin removido. Config e efeitos ficaram em $CFG (remova manualmente se quiser)."
  exit 0
fi

# --- pre-requisitos ------------------------------------------------------
say "Verificando pre-requisitos"
command -v cargo >/dev/null || die "cargo nao encontrado. Instale Rust: https://rustup.rs"
[ -d "$ROOT/crates" ] || die "voce rodou o script fora do repositorio."

# --- build ---------------------------------------------------------------
BIN="$ROOT/target/$PROFILE/soundbar-daemon"

if [ "$ACTION" != "rebuild" ] || [ ! -x "$BIN" ]; then
  say "Compilando (perfil $PROFILE) — pode demorar na primeira vez"
  (cd "$ROOT" && cargo build --"$PROFILE") || die "falha na compilacao"
fi
[ -x "$BIN" ] || die "binario do daemon nao encontrado em $BIN"

# target-triple usado no manifest (CodePaths do OpenDeck)
TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
[ -n "$TRIPLE" ] || die "nao consegui detectar o target-triple"
PLUGIN_BIN="$ROOT/target/$PROFILE/soundbar-plugin"
[ -x "$PLUGIN_BIN" ] || die "binario do plugin nao encontrado em $PLUGIN_BIN"

# --- empacotar o .sdPlugin ----------------------------------------------
say "Montando $PLUGIN_NAME.sdPlugin"
DEST="$(plugin_dir)/$PLUGIN_NAME.sdPlugin"
CFG="$(config_dir)"
mkdir -p "$CFG/sounds"

# Limpa a pasta do plugin antes de montar, para nao deixar arquivos obsoletos
# de versoes anteriores se acumulando.
if [ -d "$DEST" ]; then
  say "Limpando instalacao anterior"
  rm -rf "$DEST"
fi
mkdir -p "$DEST/assets"

# assets (manifest, icones, property inspector) vao para assets/
cp -r "$ROOT/plugin/assets/." "$DEST/assets/" || die "falha ao copiar assets"

# binario no caminho que o manifest declara para este triple
mkdir -p "$DEST/$TRIPLE/bin"
cp "$PLUGIN_BIN" "$DEST/$TRIPLE/bin/soundbar-plugin" || die "falha ao copiar o binario do plugin"
chmod +x "$DEST/$TRIPLE/bin/soundbar-plugin"

# efeitos: copiados para a config, nao para dentro do plugin
if [ -d "$ROOT/assets/sounds" ]; then
  cp -rn "$ROOT/assets/sounds/." "$CFG/sounds/" 2>/dev/null || true
fi

# --- config e sons do usuario -------------------------------------------
say "Configuracao: $CFG"

# --- instalar o daemon ---------------------------------------------------
if [ "$(os)" = "Linux" ]; then
  install -Dm755 "$BIN" "$HOME/.local/bin/soundbar-daemon"

  say "Registrando o daemon (systemd --user)"
  mkdir -p "$HOME/.config/systemd/user"
  cat > "$HOME/.config/systemd/user/soundbar.service" <<UNIT
[Unit]
Description=Sound Effects Stream Deck (audio daemon)
After=pipewire.service pulseaudio.service

[Service]
Type=simple
ExecStart=$HOME/.local/bin/soundbar-daemon run --config $CFG
Restart=on-failure
RestartSec=2

[Install]
WantedBy=default.target
UNIT

  systemctl --user daemon-reload
  systemctl --user enable --now soundbar
  sleep 1
  if systemctl --user is-active --quiet soundbar; then
    say "Daemon ativo"
  else
    warn "Daemon nao subiu. Veja: journalctl --user -u soundbar -n 30"
  fi
elif [ "$(os)" = "Darwin" ]; then
  install -Dm755 "$BIN" "$HOME/.local/bin/soundbar-daemon"
  say "Registrando o daemon (launchd)"
  mkdir -p "$HOME/Library/LaunchAgents"
  cat > "$HOME/Library/LaunchAgents/dev.soundbar.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.soundbar</string>
  <key>ProgramArguments</key>
  <array>
    <string>$HOME/.local/bin/soundbar-daemon</string>
    <string>run</string>
    <string>--config</string>
    <string>$CFG</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
PLIST
  launchctl bootout "gui/$(id -u)" "$HOME/Library/LaunchAgents/dev.soundbar.plist" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/dev.soundbar.plist"
  say "Daemon registrado"
else
  install -Dm755 "$BIN" "$HOME/.local/bin/soundbar-daemon" 2>/dev/null || cp "$BIN" "$HOME/AppData/Local/soundbar-daemon.exe"
  STARTUP="$APPDATA/Microsoft/Windows/Start Menu/Programs/Startup"
  mkdir -p "$STARTUP"
  cp "$(command -v soundbar-daemon || echo "$HOME/.local/bin/soundbar-daemon")" "$STARTUP/soundbar-daemon.exe" 2>/dev/null || true
  say "Daemon copiado para a pasta Startup (roda no login)"
fi

# --- resumo --------------------------------------------------------------
echo
say "Instalado."
cat <<EOF

  Plugin:    $DEST
  Config:    $CFG
  Sons:      $CFG/sounds

  Proximos passos:
    1. Coloque seus efeitos em $CFG/sounds
    2. No Stream Deck, arraste a acao "Play Effect" para uma tecla
    3. No OBS, adicione "Captura de saida de audio" e escolha StreamDeckSoundBar

  Teste rapido (sem o deck):   soundbar list && soundbar play teste
  Desinstalar:                 ./install.sh --uninstall

EOF
