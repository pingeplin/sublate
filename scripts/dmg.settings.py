# dmgbuild settings for the installer window: the app on the left, the arrow, and the
# Applications shortcut on the right. The background is drawn for dmgbuild's default 640x280
# window and 128pt icons; dmgbuild joins it with its @2x file into one Retina image.
import os.path

app = defines["app"]

format = "ULMO"
files = [app]
symlinks = {"Applications": "/Applications"}
background = defines["background"]
icon_locations = {os.path.basename(app): (140, 120), "Applications": (500, 120)}
