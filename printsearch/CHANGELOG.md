# Changelog

**English** · [Français](CHANGELOG.fr.md)

All notable changes to this project are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-20

### Added
- **Choice of the browser used for searches.** MakerWorld (and Thingiverse without a token) are read with a Chromium-based browser running in the background. You can now pick it in **⚙ Settings → Browser used for searches**: *Automatic*, a detected browser (**Chrome, Edge, Brave, Vivaldi, Chromium**) or a **custom path**. Chrome is no longer required: **Edge, preinstalled on Windows, is enough**.
- `printsearch browser` command to list detected browsers and select one (`auto`, `chrome`, `edge`, `brave`, `vivaldi`, `chromium` or a path).
- **Application icon embedded in the Windows `.exe`** (`build.rs`, `packaging/printsearch.ico`). If the icon cannot be embedded (for example `windres` missing when cross-compiling), the build still succeeds and prints a warning.
- **Progressive results:** each site is queried separately, so fast sites (e.g. Printables) appear immediately instead of waiting for the slowest one. A progress message shows how many sites have answered, and an error on one site no longer hides the others.
- `printsearch --version`.
- This changelog, in English and French.

### Changed
- Results are merged client-side (sites interleaved, then sorted by likes or downloads when requested). Result cards already on screen are reused: no flicker, and their entrance animation is no longer replayed. The same model appearing twice is shown once.
- A single HTTP client is shared by the whole program (connection reuse) and now has timeouts: 8 s to connect, 25 s per request.
- The page cache of the browser reader is capped (40 entries) and expired entries are purged.
- The configuration file is written atomically (temporary file then rename).
- The `.deb` package now recommends Edge and Brave as well as Chromium and Chrome.
- The maintainer and author of the package are now **Print-lab**.
- READMEs updated (browser choice, Windows icon, cross-compilation from Linux, `rustfmt` requirement for building).

### Security
- The local server now rejects any request whose `Host` is not `localhost:8787` / `127.0.0.1:8787`, and any request with a foreign `Origin`. This protects against malicious web pages opened in your browser (CSRF and DNS rebinding), which could otherwise have called the local API, for example to change the browser executable.

### Fixed
- **Hang when the selected browser is not a real browser** (wrong path, non-Chromium program): the launch could block forever and freeze later searches. Launch now fails after 35 s with a clear message, a failed launch is remembered for 20 s instead of being retried in a loop, each browser call has a 10 s timeout, and a page read is capped at 75 s.
- A search started with no site selected could still display the results of the previous search.
- A panic in one thread can no longer permanently disable the browser reader (poisoned locks are recovered).
- Starting a second instance now reports a clear message when port 8787 is already in use.

### Notes
- Firefox and Safari cannot be used for searches (the reader needs a Chromium-based browser).
- Building requires `rustfmt` (included in a normal `rustup` install) because of the `headless_chrome` dependency.
- Unchanged limitations: the Printables and MakerWorld endpoints are unofficial, and Like / collections remain experimental (see the README).

## [0.1.0]

Initial release.

### Added
- Search across **Thingiverse, Printables and MakerWorld** from a local web interface and a command-line mode.
- Filters (sites, price, sort, minimum likes and downloads, results per site) with automatic re-search, pagination ("Load more") and animations.
- Like and add-to-collection actions (experimental), connection to sites from a Settings page (Thingiverse App Token or OAuth, Printables Bearer token).
- English and French interface (English by default) and a search history that can be turned off.
- Packaging: `.deb` metadata, launcher and icon; Windows build. Released under the GPL v3.

[0.2.0]: https://github.com/Print-lab/PrintSearch/releases/tag/v0.2.0
[0.1.0]: https://github.com/Print-lab/PrintSearch/releases/tag/v0.1.0
