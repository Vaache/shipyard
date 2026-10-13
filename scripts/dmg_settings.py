import os

app = defines.get("app", "build/Shipyard.app")
readme = defines.get("readme", "Resources/DMG/How to Install.rtf")
app_name = os.path.basename(app)
readme_name = os.path.basename(readme)

format = "UDZO"
size = None
files = [app, readme]
symlinks = {"Applications": "/Applications"}
icon = "Resources/AppIcon.icns"
background = "Resources/DMG/background.png"

show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
sidebar_width = 0
default_view = "icon-view"
show_icon_preview = False
include_icon_view_settings = True
include_list_view_settings = False

window_rect = ((200, 140), (660, 480))
arrange_by = None
grid_offset = (0, 0)
grid_spacing = 100
scroll_position = (0, 0)
label_pos = "bottom"
text_size = 13
icon_size = 96

icon_locations = {
    app_name: (165, 170),
    "Applications": (495, 170),
    readme_name: (330, 268),
}

hide_extensions = [app_name]
