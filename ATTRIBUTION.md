# Attribution

Every non-code asset and bundled third-party item in this repository, with its author, source and
licence. Milleleaf's own code and original assets are MIT OR Apache-2.0 (see
[`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE) and [`NOTICE`](NOTICE)); the
Milleleaf name and logo are not (see [`brand/LICENSE-brand.txt`](brand/LICENSE-brand.txt)). When you
add an asset, add a row here in the same change; third-party material must be permissively licensed
and keep its licence file next to it or in [`licenses/`](licenses/).

Licences were checked on 22 October 2026 against each source (its licence file at the pinned
revision, the font's name table, or the record's licence field). "Unverified" says where that wasn't
possible.

## Built into the `ox` program

| Path | What | Author | Source | Licence | Licence file |
|---|---|---|---|---|---|
| WordCraft `wordcraft-fonts` (git rev `eae2421`), compiled in | Inter, JetBrains Mono, Source Sans 3, Source Serif 4 (13 TTFs) | The Inter Project Authors; The JetBrains Mono Project Authors; Adobe | <https://github.com/storytold/wordcraft> (`assets/fonts`), from <https://github.com/rsms/inter>, <https://github.com/JetBrains/JetBrainsMono>, <https://github.com/adobe-fonts> | OFL-1.1 | [`licenses/OFL-Inter.txt`](licenses/OFL-Inter.txt), [`OFL-JetBrainsMono.txt`](licenses/OFL-JetBrainsMono.txt), [`OFL-SourceSans3.txt`](licenses/OFL-SourceSans3.txt), [`OFL-SourceSerif4.txt`](licenses/OFL-SourceSerif4.txt) (copied from WordCraft at that rev) |
| WordCraft `wordcraft-proof`, compiled in | English hyphenation dictionary (`en-us.dic`, Moby Hyphenator II) | Grady Ward | <https://www.gutenberg.org/ebooks/3204> | Public domain | none needed |
| PDFCraft `pdfcraft-fonts` (git rev `bc7a12b`) | Dancing Script (variable), Inter Regular | The Dancing Script Project Authors; The Inter Project Authors | <https://github.com/storytold/pdfcraft>, from <https://github.com/googlefonts/DancingScript> | OFL-1.1 | [`licenses/OFL-DancingScript.txt`](licenses/OFL-DancingScript.txt), [`licenses/OFL-Inter.txt`](licenses/OFL-Inter.txt). Unverified whether the linker keeps them in `ox` |
| `webpki-root-certs` 1.0.9 (dependency of `ox-cli`) | Mozilla's root certificates (CCADB) | The rustls project, from Mozilla | <https://github.com/rustls/webpki-roots> | CDLA-Permissive-2.0 | [`licenses/CDLA-Permissive-2.0-webpki-root-certs.txt`](licenses/CDLA-Permissive-2.0-webpki-root-certs.txt) (the crate's own `LICENSE`) |
| `krilla` (in craft-libs rev `82a24b0`), compiled in | Compact ICC profiles (sRGB, sGrey; v2 and v4) | Clinton Ingram | <https://github.com/saucecontrol/Compact-ICC-Profiles> | CC0-1.0 | none needed |

Other Rust and JavaScript dependencies are MIT, Apache-2.0 or both, and carry their own licence files
in their packages; deckcraft, gridcraft and craft-pdf embed no fonts at the pinned revisions (the
craft-fonts set is used only when `CRAFT_FONTS_DIR` is set, which nothing here does).

## Website (`site/`)

| Path | What | Author | Source | Licence | Licence file |
|---|---|---|---|---|---|
| `site/public/fonts/commissioner-variable.ttf` | Commissioner, variable, version 1.001 | The Commissioner Project Authors (Kostas Bartsokas) | <https://github.com/kosbarts/Commissioner> | OFL-1.1 | [`site/public/fonts/OFL-Commissioner.txt`](site/public/fonts/OFL-Commissioner.txt) |
| `site/public/fonts/ibm-plex-sans-regular.ttf`, `ibm-plex-sans-semibold.ttf` | IBM Plex Sans, version 3.201 (not referenced by the site today) | IBM Corp. | <https://github.com/IBM/plex> | OFL-1.1 | [`site/public/fonts/OFL.txt`](site/public/fonts/OFL.txt) |
| `site/public/brand/*.svg`, `brand/*` | Milleleaf logo, dark logo, mark and avatar | Helen Kwok | Original work | Milleleaf brand terms (not open source) | [`brand/LICENSE-brand.txt`](brand/LICENSE-brand.txt) |

## Examples and fixtures

| Path | What | Author | Source | Licence | Licence file |
|---|---|---|---|---|---|
| `examples/real/paper.docx` | "10 Simple rules for design, provision, and reuse of identifiers for web-based life science data" (manuscript) | Julie McMurry et al. (36 authors) | <https://doi.org/10.5281/zenodo.31765> | CC-BY-4.0 | [`examples/real/LICENSE-CC-BY-4.0.txt`](examples/real/LICENSE-CC-BY-4.0.txt); attribution in [`examples/real/SOURCES.md`](examples/real/SOURCES.md) |
| `examples/real/lca-model.xlsx` | Sodium-ion battery LCA results workbook (ESI-2) | Jens F. Peters, Manuel Baumann, Joachim Binder, Marcel Weil | <https://doi.org/10.5281/zenodo.4742246> | CC-BY-4.0 | as above |
| `examples/real/lcoh-calculator.xlsx` | LCOH calculator | Wilson Ricks, Jesse Jenkins | <https://doi.org/10.5281/zenodo.7948769> | CC-BY-4.0 | as above |
| `examples/real/fixtures/paper.*`, `lca-model.*` | Milleleaf's export of the two files above, re-saved or edited in LibreOffice 24.2 and Microsoft Office 16 (modified copies) | The original authors; modified by the Milleleaf contributors | Derived from the rows above; each change in `SOURCES.md` | CC-BY-4.0 | as above |
| `examples/real/milleleaf-*`, `examples/real/fixtures/milleleaf-deck.*`, `examples/real/authored/*` (incl. `chart.jpg`) | Our own deck and workbooks, written by `authored/*.py` and saved by real apps; `chart.jpg` is a slide `chart_slide.py` writes and LibreOffice renders | Milleleaf contributors | Original work, see [`examples/real/SOURCES.md`](examples/real/SOURCES.md) | MIT OR Apache-2.0 (the embedded Milleleaf mark: brand terms) | root licences |
| `examples/hello-table/fixtures/*` | Re-saves and edits of Milleleaf's export of the hello-table example, made in LibreOffice, Microsoft Office and openpyxl | Milleleaf contributors | Original work | MIT OR Apache-2.0 | root licences |
| `examples/hello-table/assets/842c3244c641a24d.png` | A line chart picture (720×320) | Milleleaf contributors (unverified: in the first commit, no script or note names its maker) | — | MIT OR Apache-2.0 (assumed) | root licences |

## Test vectors and data

| Path | What | Author | Source | Licence | Licence file |
|---|---|---|---|---|---|
| `crates/ox-store/testdata/pq-vectors.json` (ML-DSA, SLH-DSA part) | Subset of NIST ACVP-Server FIPS 204/205 vectors, picked by `pq-vectors-subset.py` | NIST | <https://github.com/usnistgov/ACVP-Server> at `975de31` | NIST software notice (public domain in the US; keep the notice, acknowledge NIST, mark changes) | [`crates/ox-store/testdata/NOTICE-NIST.txt`](crates/ox-store/testdata/NOTICE-NIST.txt) |
| `crates/ox-store/testdata/pq-vectors.json` (Wycheproof part) | ML-DSA-65 verify and sign-seed vectors | Wycheproof authors (C2SP) | <https://github.com/C2SP/wycheproof> at `7a32593` | Apache-2.0 | [`LICENSE-APACHE`](LICENSE-APACHE) |
| `crates/ox-seal/vectors/xwing-hkdfsha256-chacha.json` | X-Wing + HKDF-SHA256 + ChaCha20-Poly1305 HPKE test vector | IETF Trust and the draft-ietf-hpke-pq authors | <https://github.com/hpkewg/hpke-pq> at `6433c8f` (also shipped in the `hpke` 0.14.1 crate, MIT OR Apache-2.0) | **Unverified**: the repository points to the IETF Trust Legal Provisions, under which code components are BSD-3-Clause | none yet |
| `crates/ox-store/testdata/ots/*`, `crates/ox-store/testdata/headers/*` | Our random bytes and their OpenTimestamps proof (calendars alice/bob.btc.calendar.opentimestamps.org, finney.calendar.eternitywall.com, btc.calendar.catallaxy.com); Bitcoin block headers and retarget table | Milleleaf contributors; calendar attestations and block headers are public chain data | Public calendars and the Bitcoin chain | Original work / factual data | none needed |
| `crates/ox-store/testdata/tsa/*` | RFC 3161 response from our own local test TSA | Milleleaf contributors | Original work | MIT OR Apache-2.0 | root licences |
| `schema/ox.tsp` | OX layer schema, derived from IFC5's `ifcx.tsp` | buildingSMART International (the original); Milleleaf contributors (changes) | <https://github.com/buildingSMART/IFC5-development> (`schema/ifcx.tsp`) | **Unknown**: the upstream repository gives no licence for the schema | none |

## Tools installed for checks (not bundled)

`tools/ci/doclang-requirements.txt` (the DocLang validator in CI) and
`examples/real/authored/requirements.txt` (the authoring scripts) install Python packages; none of them
ships in the repository or in `ox`. Their licences (PyPI metadata, 22 October 2026): doclang
Apache-2.0; lxml BSD-3-Clause; click BSD-3-Clause; rich MIT; Pygments BSD-2-Clause; markdown-it-py
MIT; mdurl MIT; annotated-doc MIT; typer MIT; shellingham ISC; saxonche (no licence in its metadata;
SaxonC-HE is published by Saxonica as MPL-2.0, unverified); python-pptx MIT; openpyxl MIT; et_xmlfile
MIT; Pillow MIT-CMU; typing_extensions PSF-2.0; XlsxWriter BSD-2-Clause.
