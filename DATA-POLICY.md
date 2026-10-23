# Data policy

Milleleaf collects nothing about you. It has no accounts, no telemetry, no crash reports, no analytics and no update check. Your files stay where you keep them.

## The command-line tool and libraries

- **Your files stay where you keep them:** on your machine or wherever your project lives, such as a folder, a NAS, a USB drive or a cloud folder you chose. Milleleaf needs no server of its own and sends your files nowhere.
- **Other apps run on your machine.** When you ask Milleleaf to render, convert or open a file, it starts LibreOffice or an ArtCraft app as a local program. Nothing goes over the network.
- **The network is used only when you ask for it, and only to services you choose.** Each of these reveals your IP address to the service you contact:

| You run | What is sent | To whom |
|---|---|---|
| `mleaf timestamp` with `--tsa URL` | A hash of the layer's signature file (RFC 3161), never the file | The time-stamp authority you named |
| `mleaf timestamp` with `--ots CALENDAR` | A hash of the signature file's hash plus a random value, so the calendar learns nothing about the file | The OpenTimestamps calendars you named |
| `mleaf timestamp --upgrade` | A request for the finished proof | The same calendars |
| `mleaf verify --online` | Requests for public Bitcoin block headers | mempool.space and blockstream.info by default, or the sources (or your own Bitcoin Core node) listed in your project's `.mleaf/policy.json` |
| `milleleaf-headers sync` | Requests for public Bitcoin block headers | Bitcoin network peers, or the peers you name with `--peer`, optionally through a SOCKS5 proxy (`--socks5`) |

There is no default time-stamp service: nothing is time-stamped until you name one.

## The website and its demos

- **The site is static files** served by Cloudflare. Like any web host, Cloudflare handles each request (your IP address and browser details) to deliver the page. Cloudflare request logging and Web Analytics are off for the site, and the site loads no third-party scripts, fonts or trackers.
- **Light or dark theme:** if you pick one, your browser remembers it (`localStorage`, key `milleleaf-theme`). It never leaves your browser.
- **The demos run in your browser.** Files you drop are read and processed on your own device by the document engine, which runs as WebAssembly. The demo pages' security policy blocks every network connection after the page loads (`connect-src 'none'`), so a file cannot be sent anywhere. Automated checks confirm it: they load a demo, go offline, and run every operation, with zero network requests after loading.

## Questions and changes

Questions about privacy: hello@milleleaf.com.

Any change to this policy is made in this file, so its history is public.
