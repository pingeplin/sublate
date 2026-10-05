# dmgbuild settings for the installer window: the app on the left, an arrow, and the
# Applications shortcut on the right. The icon positions suit dmgbuild's default 640x280
# window and 128pt icons, which its built-in arrow background is drawn for.
import os.path

app = defines["app"]

format = "ULMO"
files = [app]
symlinks = {"Applications": "/Applications"}
background = "builtin-arrow"
icon_locations = {os.path.basename(app): (140, 120), "Applications": (500, 120)}
