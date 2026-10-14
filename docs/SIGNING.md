# Code signing (Windows SmartScreen)

Unsigned builds work, but Windows SmartScreen warns on first run until
the binary gains reputation (downloads + time). A signature fixes the
"unknown publisher" part immediately; full trust still builds up as
people install new versions. Keep asset filenames stable
(`WallMotion-Settings.exe`, `WallMotion-Setup-*.exe`) — reputation
sticks to names.

## Free signing for open source: SignPath

Paid certificates cost hundreds per year. For public OSS repos,
[SignPath](https://signpath.io/) signs release builds for free:

1. Register at signpath.io and request free OSS signing.
2. Create a project (e.g. slug `wallmotion`) with an artifact
   configuration for Windows exes (e.g. slug `wallmotion-exe`).
3. Create an API token (needs submit rights for that project).
4. Add repo secrets (Settings → Secrets → Actions):
   - `SIGNPATH_API_TOKEN` — the token,
   - `SIGNPATH_ORG_ID` — your organization ID from SignPath.
5. If your slugs differ, edit them in `.github/workflows/sign.yml`.
6. After a GitHub Release is published, run the **Sign release**
   workflow (Actions tab) with the tag (e.g. `v1.1.5`). It downloads
   the release exes, submits them, waits, and re-uploads the signed
   files over the originals.

Notes and honest caveats:

- The `Sign release` workflow never runs on its own and does nothing
  without the secrets above; the normal tag pipeline stays green.
- A signature does **not** grant instant SmartScreen trust — it still
  helps (known publisher) and compounds with the installer (proper
  metadata + uninstaller) and download counts over releases.
- Self-signed certificates do **not** help SmartScreen at all;
  don't bother for distribution (they're fine for local testing only).
- `signtool verify /pa` on the downloaded exe shows the signature.
- Linux needs no signing (distro package managers + checksums cover
  trust there).
