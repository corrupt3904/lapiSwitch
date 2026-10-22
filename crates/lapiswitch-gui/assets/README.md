# GUI assets

- `tb520fu.png`, `generic_tablet.svg`: copied unchanged from LTBox v3.3.4 (730b30d), GPL-3.0-or-later, for the same Dashboard portrait.
- `flags/`: flag-icons 7.3.2 SVGs copied from LTBox, MIT; license in `flags/LICENSE`.
- `flags/ww.svg`: locally drawn globe, GPL-3.0-or-later; the fallback for a code without a flag.
- The country list is LTBox’s country picker (89 codes, `ltbox-gui` `COUNTRY_CODES`); the backend accepts exactly these as new codes.

- `branding/lapiswitch-source.png`: original AI-generated lapiSwitch icon (light gray rounded tile, red upper / blue lower S-arrow).
- `branding/lapiswitch.png`, `branding/lapiswitch.ico`: resized/packaged derivatives of that source for the GUI/window and Windows executable. Regenerate with `cargo run -p lapiswitch-tools -- icons`; no recoloring, redrawing or external assets are involved.

- `settings.svg`: locally drawn gear for the settings button, GPL-3.0-or-later.

- `licenses.json`: offline inventory and verbatim license/copyright texts for GUI normal/build dependencies and bundled assets. Identical texts are stored once and referenced by each component; settings exposes every document, including the project's GPL and third-party notices. Refresh this inventory when dependency versions or bundled assets change.

- The TWRP / TeamWin catalog entry identifies the separately downloaded lapis release `v3.7.1_16-20261014`, image SHA-256, release/device-tree/source references and credits. It preserves the recovery source's copyright/GPL NOTICE and Apache-2.0 notices for marked device-tree files. Other components of the upstream recovery image retain their respective licenses; the entry does not relabel the complete image as GPL-only.
