# Dockspace project site

GitHub Pages serves this directory from `main` → `/docs` at https://cybercore-tech.github.io/dockspace/.

Local preview: `python -m http.server 8792 --bind 127.0.0.1` from this directory, then open http://127.0.0.1:8792.

- `index.html`: project content and signature Cybergrid theme recipes.
- `app.js`, `styles.css`: existing Cybercore theme engine and shared presentation kit.
- `dockspace.css`: Dockspace's control-room layout and responsive styling.
- `playground.js`: three sample stacks, simulated actions/logs, sample Compose checker, session activity, and Ctrl/Cmd+K navigation.
- `assets/`: Dockspace brand mark and locally bundled fonts with their licenses.

Themes load from the shared registry at https://cybercore-tech.github.io/data. The visual fallback remains available offline.

The playground runs entirely in memory. It does not contact Docker, save files, or execute commands. The Compose checker checks only the demonstrated sample fields; it does not parse arbitrary YAML or replace the real application's `docker compose config` validation.

Publish by pushing these `docs/` changes to the repository's `main` branch, with GitHub Pages configured to deploy from `main` / `docs`.
