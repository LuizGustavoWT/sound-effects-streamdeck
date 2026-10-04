#!/bin/bash
set -e
# ---------- Configuration ----------
BINARY_NAME="soundbar-plugin"
PLUGIN_FOLDER="com.soundbar.streamdeck.sdPlugin"
# ---------- Usage ----------
if [ $# -ne 1 ]; then
    echo "Usage: $0 {debug|release}"
    exit 1
fi
MODE="$1"
# ---------- Set paths based on mode ----------
if [ "$MODE" = "debug" ]; then
    BINARY_PATH="target/debug/$BINARY_NAME"
    ZIP_NAME="com.soundbar.streamdeck-debug.zip"
elif [ "$MODE" = "release" ]; then
    BINARY_PATH="target/release/$BINARY_NAME"
    ZIP_NAME="com.soundbar.streamdeck.zip"
else
    echo "Invalid mode: $MODE (use 'debug' or 'release')"
    exit 1
fi
# ---------- Build ----------
echo "🔨 Compilando em modo $MODE..."
if [ "$MODE" = "debug" ]; then
    cargo build -p streamdeck-ffi
elif [ "$MODE" = "release" ]; then
    cargo build --release -p streamdeck-ffi
fi
# ---------- Check that the binary exists ----------
if [ ! -f "$BINARY_PATH" ]; then
    echo "❌ Erro: binário não encontrado em $BINARY_PATH"
    exit 1
fi
# ---------- Check required source files ----------
if [ ! -f "src/manifest.json" ]; then
    echo "❌ Erro: src/manifest.json não encontrado"
    exit 1
fi
if [ ! -d "src/assets" ]; then
    echo "❌ Erro: src/assets não encontrado"
    exit 1
fi
# ---------- Create temporary packaging directory ----------
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT
# ---------- Create the required parent folder ----------
mkdir -p "$TMP_DIR/$PLUGIN_FOLDER"
# ---------- Copy files into the plugin folder ----------
cp "$BINARY_PATH" "$TMP_DIR/$PLUGIN_FOLDER/"
cp src/manifest.json "$TMP_DIR/$PLUGIN_FOLDER/"
cp -r src/assets "$TMP_DIR/$PLUGIN_FOLDER/"
if [ -d "src/propertyInspector" ]; then
    cp -r src/propertyInspector "$TMP_DIR/$PLUGIN_FOLDER/"
fi
# ---------- Create zip ----------
rm -f "$ZIP_NAME"
cd "$TMP_DIR"
zip -r "$OLDPWD/$ZIP_NAME" "$PLUGIN_FOLDER" > /dev/null
cd - > /dev/null
echo "✅ Zip criado: $ZIP_NAME"
echo "   Conteúdo: $PLUGIN_FOLDER/"
unzip -l "$ZIP_NAME" | head -20
