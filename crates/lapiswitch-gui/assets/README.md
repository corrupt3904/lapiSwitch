# GUI assets

- `tb520fu.png`, `generic_tablet.svg`: copied unchanged from LTBox v3.3.4 (730b30d), GPL-3.0-or-later, for the same Dashboard portrait.
- `flags/`: flag-icons 7.3.2 SVGs copied from LTBox, MIT; license in `flags/LICENSE`.
- `flags/ie.svg`, `flags/lu.svg`, `flags/mt.svg`: simple locally drawn SVG flags for canonical patcher codes absent from LTBox’s bundled GUI flags. These follow this project’s GPL-3.0-or-later license.
- `flags/ww.svg`: locally drawn globe for Worldwide (WW), GPL-3.0-or-later.
- The country list is limited to LTBox’s canonical patcher codes. This prevents the GUI offering a country that the backend cannot accept.

- `branding/lapiswitch-source.png`: original AI-generated lapiSwitch icon (light gray rounded tile, red upper / blue lower S-arrow).
- `branding/lapiswitch.png`, `branding/lapiswitch.ico`: resized/packaged derivatives of that source for the GUI/window and Windows executable. Regenerate with `cargo run -p lapiswitch-tools -- icons`; no recoloring, redrawing or external assets are involved.

- `settings.svg`: locally drawn gear for the settings button, GPL-3.0-or-later.

- `licenses.json`: offline inventory and verbatim license/copyright texts for GUI normal/build dependencies and bundled assets. Identical texts are stored once and referenced by each component; settings exposes every document, including the project's GPL and third-party notices. Refresh this inventory when dependency versions or bundled assets change.
