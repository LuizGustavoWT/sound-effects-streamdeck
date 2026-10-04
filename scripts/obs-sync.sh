#!/usr/bin/env bash
# Mantem a captura de audio do OBS apontada para o sink do soundbar.
#
# Por que isso e necessario: a fonte "Audio de Saida" do OBS guarda o NOME
# do dispositivo, nao um id estavel. Quando o daemon reinicia, o sink e
# recriado e o OBS pode ficar sem captura (ou apontado para o monitor, que
# nao e a saida). Este script corrige o nome sem precisar reabrir o OBS.
#
# Uso:
#   ./scripts/obs-sync.sh            # corrige a fonte
#   ./scripts/obs-sync.sh --check    # so diz o que esta selecionado agora
set -euo pipefail

SINK_NAME="${SOUNDBAR_SINK:-StreamDeckSoundBar}"
OBS_DIR="${OBS_CONFIG_DIR:-$HOME/.config/obs-studio}"

CHECK_ONLY=0
[ "${1:-}" = "--check" ] && CHECK_ONLY=1

say() { printf '==> %s\n' "$*"; }
warn() { printf '[!] %s\n' "$*" >&2; }
die() { printf '[x] %s\n' "$*" >&2; exit 1; }

# Confere que o sink existe, e devolve o nome correto.
current_sink() {
    pactl list short sinks 2>/dev/null \
        | awk -v n="$SINK_NAME" '$2 == n { print $2; exit }'
}

if ! SINK="$(current_sink)"; then
    die "sink '$SINK_NAME' nao existe. O daemon esta rodando?"
fi

say "sink disponivel: $SINK"

SCENES="$OBS_DIR/basic/scenes"
[ -d "$SCENES" ] || die "OBS nao encontrado em $SCENES"

if [ "$CHECK_ONLY" = "1" ]; then
    found=0
    for f in "$SCENES"/*.json; do
        [ -f "$f" ] || continue
        python3 - "$f" "$SINK" <<'PY'
import json, sys, os
path, sink = sys.argv[1], sys.argv[2]
d = json.load(open(path))
hits = []

def check(sid, src):
    if not isinstance(src, dict):
        return
    if src.get("id") in ("pulse_output_capture", "wasapi_output_capture",
                         "pulse_output_capture_adv", "coreaudio_output_capture",
                         "wasapi_output_capture_adv"):
        dev = (src.get("settings") or {}).get("device_id")
        hits.append((sid, src.get("name"), dev))

for key in ("DesktopAudioDevice1", "AuxAudioDevice1"):
    check(key, d.get(key))
for s in d.get("sources", []) or []:
    check(s.get("id"), s)

if hits:
    print(f"{os.path.basename(path)}:")
    for sid, name, dev in hits:
        ok = "OK" if dev == sink else "DIVERGENTE"
        print(f"  [{ok}] {name} (id={sid}) device_id={dev!r}")
PY
    done
    exit 0
fi

# Corrige: substitui device_id pela saida (sem o sufixo .monitor).
CHANGED=0
for f in "$SCENES"/*.json; do
    [ -f "$f" ] || continue
    if python3 - "$f" "$SINK" <<'PY'
import json, sys, os, tempfile
path, sink = sys.argv[1], sys.argv[2]
d = json.load(open(path))
changed = False

def fix(src):
    global changed
    if not isinstance(src, dict):
        return
    if src.get("id") in ("pulse_output_capture", "wasapi_output_capture",
                         "pulse_output_capture_adv", "coreaudio_output_capture",
                         "wasapi_output_capture_adv"):
        st = src.setdefault("settings", {})
        if st.get("device_id") != sink:
            st["device_id"] = sink
            changed = True

for key in ("DesktopAudioDevice1", "AuxAudioDevice1"):
    fix(d.get(key))
for s in d.get("sources", []) or []:
    fix(s)

if changed:
    # escrita atomica: o OBS le esses arquivos ao abrir
    tmp = path + ".synctmp"
    with open(tmp, "w") as fh:
        json.dump(d, fh, ensure_ascii=False, indent=2)
    os.replace(tmp, path)

sys.exit(0 if changed else 1)
PY
    then
        say "atualizado: $(basename "$f")"
        CHANGED=1
    fi
done

if [ "$CHANGED" = "1" ]; then
    say "feito. Feche e abra o OBS para aplicar."
else
    say "ja estava correto em todos os perfis."
fi
