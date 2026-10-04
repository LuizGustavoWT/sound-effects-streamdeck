#!/usr/bin/env python3
"""Dialogo nativo para escolher um arquivo de audio.

Usado pelo plugin quando voce clica em "Escolher arquivo..." no Property
Inspector. Imprime o caminho escolhido em stdout (vazio se cancelar).

Nao depende de zenity/kdialog: usa GTK via PyGObject, que ja vem no GNOME.
"""
import os
import subprocess
import sys

AUDIO_PATTERNS = [
    "*.wav", "*.mp3", "*.flac", "*.ogg", "*.oga", "*.opus",
    "*.aiff", "*.aif", "*.m4a", "*.aac", "*.wma",
]


def pick_with_gtk():
    import gi
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk

    dialog = Gtk.FileChooserDialog(
        title="Escolha um efeito sonoro",
        action=Gtk.FileChooserAction.OPEN,
    )
    dialog.add_buttons(
        "Cancelar", Gtk.ResponseType.CANCEL,
        "Importar", Gtk.ResponseType.OK,
    )
    dialog.set_default_response(Gtk.ResponseType.OK)

    audio = Gtk.FileFilter()
    audio.set_name("Audio (wav, mp3, flac, ogg...)")
    for p in AUDIO_PATTERNS:
        audio.add_pattern(p)
    dialog.add_filter(audio)

    todos = Gtk.FileFilter()
    todos.set_name("Todos os arquivos")
    todos.add_pattern("*")
    dialog.add_filter(todos)

    # Comeca na pasta de sons, se existir.
    start = os.environ.get("SOUNDBAR_SOUNDS_DIR")
    if start and os.path.isdir(start):
        try:
            dialog.set_current_folder(start)
        except Exception:
            pass

    response = dialog.run()
    chosen = dialog.get_filename() if response == Gtk.ResponseType.OK else None
    dialog.destroy()
    return chosen


def pick_with_zenity():
    """Fallback para quem tem zenity instalado."""
    patterns = " ".join(AUDIO_PATTERNS)
    cmd = [
        "zenity", "--file-selection",
        f"--title=Escolha um efeito sonoro",
        f"--file-filter=Audio | {patterns}",
        "--file-filter=Todos | *",
    ]
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    except Exception:
        return None
    return out.stdout.strip() or None


def main():
    # GTK precisa de display; sem ele, tenta o zenity.
    has_display = bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"))
    chosen = None
    if has_display:
        try:
            chosen = pick_with_gtk()
        except Exception as e:
            print(f"aviso: GTK falhou ({e}), tentando zenity", file=sys.stderr)
    if chosen is None and shutil_available("zenity"):
        chosen = pick_with_zenity()
    print(chosen or "")
    return 0


def shutil_available(cmd):
    from shutil import which
    return which(cmd) is not None


if __name__ == "__main__":
    sys.exit(main())
