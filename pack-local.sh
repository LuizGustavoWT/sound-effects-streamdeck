#!/bin/bash
set -e

# Build the plugin and install it straight into OpenDeck's plugin folder.
# No git, no GitHub release — just build + copy, so you can iterate quickly.
#
#   ./pack-local.sh          release build (default)
#   ./pack-local.sh debug    faster debug build

MODE="${1:-release}"

BINARY_NAME="soundbar-plugin"
PLUGIN_UUID="com.soundbar.streamdeck.sdPlugin"

# Flatpak (OpenDeck via Flathub) e nativo — cobre ambos os casos.
FLATPAK_DEST="$HOME/.var/app/me.amankhanna.opendeck/config/opendeck/plugins/$PLUGIN_UUID"
NATIVE_DEST="$HOME/.config/opendeck/plugins/$PLUGIN_UUID"
NATIVE_DEST2="$HOME/.config/streamdeck/plugins/$PLUGIN_UUID"

if [ "$MODE" = "debug" ]; then
    BUILD_DIR="target/debug"
else
    BUILD_DIR="target/release"
fi

echo "🔨 Compilando em modo $MODE..."
if [ "$MODE" = "debug" ]; then
    cargo build -p streamdeck-ffi
else
    cargo build --release -p streamdeck-ffi
fi

BIN_SRC="$BUILD_DIR/$BINARY_NAME"
if [ ! -f "$BIN_SRC" ]; then
    echo "❌ Erro: binário não encontrado em $BIN_SRC"
    exit 1
fi

# OpenDeck keeps the plugin running, and Linux refuses to overwrite a binary
# that is currently executing ("Text file busy"). Stop it first.
echo "🛑 Parando o plugin em execução (se houver)..."
pkill -f "$PLUGIN_UUID/$BINARY_NAME" 2>/dev/null || true
pkill -f "soundbar-plugin" 2>/dev/null || true
for _ in $(seq 1 20); do
    if pgrep -f "$BINARY_NAME" >/dev/null 2>&1; then
        sleep 0.1
    else
        break
    fi
done

install_into() {
    local DEST="$1"
    [ -d "$(dirname "$DEST")" ] || return 0
    echo "📦 Instalando em: $DEST"
    mkdir -p "$DEST/assets" "$DEST/propertyInspector"

    # Copia o binário por cima do antigo (substituição atômica seria melhor,
    # mas aqui o processo já foi morto acima).
    cp -f "$BIN_SRC" "$DEST/$BINARY_NAME"
    chmod +x "$DEST/$BINARY_NAME"

    cp src/manifest.json "$DEST/"

    cp -r src/assets/. "$DEST/assets/"
    cp -r src/propertyInspector/. "$DEST/propertyInspector/"
}

FOUND=0
install_into "$FLATPAK_DEST" && FOUND=1
install_into "$NATIVE_DEST" && FOUND=1
install_into "$NATIVE_DEST2" && FOUND=1

if [ "$FOUND" = "0" ]; then
    echo ""
    echo "⚠️  Nenhuma pasta de plugins do OpenDeck encontrada. Procurou:"
    echo "   $FLATPAK_DEST"
    echo "   $NATIVE_DEST"
    echo "   $NATIVE_DEST2"
    echo ""
    echo "   Abra o OpenDeck uma vez (para criar a pasta), ou rode ./install.sh"
    exit 1
fi

echo "✅ Plugin atualizado localmente!"
echo "🔄 Reinicie o OpenDeck para aplicar (feche e abra de novo)."
echo "   Devices e ações novas exigem restart; mudanças de imagem às vezes não."
