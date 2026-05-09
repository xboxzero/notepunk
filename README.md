# NotePunk

Beat-generation punk note capture. Free, open-source, no accounts.

> capture // remix // remember

## What

A note-taking site that wants to be felt as much as used. Typewriter type, ink-on-paper, the speed of a Kerouac scroll. The plumbing underneath is modern: Rust compiled to WebAssembly, runs entirely in the browser, your notes never leave the device.

## Stack

- **Core:** Rust → WebAssembly (Leptos + Trunk)
- **Graph view:** Cytoscape.js, force-directed, Obsidian-style backlinks
- **Image search:** Wikimedia Commons (no API key, free-use images)
- **Voice capture:** in-browser Whisper STT (tiny model, ~75MB, offline after first load)
- **Storage:** browser localStorage; export to Markdown
- **Hosting:** GitHub Pages, deployed via GitHub Actions

## Status

Phase 1 — scaffold. Live at <https://xboxzero.github.io/notepunk/>.

Roadmap:

1. Scaffold (Rust + Trunk + Pages deploy) ← **here**
2. Core notepad — beat-punk skin, save/load, Markdown export
3. Wikilinks + Cytoscape graph view
4. Wikimedia image search + inline insert
5. Whisper STT from audio recorder
6. Beat-gen / capture-focused guide

## License

MIT.
