# KoeServe

A lightweight Japanese TTS HTTP server built with Rust, JPreprocess, and JBonsai.

KoeServe is an independent project and is not affiliated with or endorsed by the JPreprocess organization.

## Current features

- Axum HTTP server
- OpenAPI + Swagger UI
- JPreprocess with bundled NAIST-JDIC
- JBonsai `.htsvoice` loading at startup
- All discovered voices kept resident
- Dedicated bounded CPU worker pool
- Per-request `speed`, `pitch`, `alpha`, `beta`, and `volume_db`
- WAV output
- Voice/model defaults exposed by `GET /v1/voices`

## Models

KoeServe loads voice models from `models/manifest.toml`. Voice discovery is explicit rather than based on file names. This keeps API voice IDs stable even when the underlying file names differ.

Example:

```toml
version = 1

[[voices]]
id = "mei_normal"
speaker = "mei"
style = "normal"
display_name = "Mei Normal"
path = "mei/mei_normal.htsvoice"
enabled = true

[[voices]]
id = "mei_happy"
speaker = "mei"
style = "happy"
display_name = "Mei Happy"
path = "mei/mei_happy.htsvoice"
enabled = true
```

Paths are resolved relative to `models/manifest.toml`. The `models/` directory is therefore a self-contained model store containing the manifest, voice files, and retained license notices. Absolute paths are also accepted. Set `enabled = false` to keep an entry in the manifest without loading it.

At startup KoeServe validates the manifest and fails early when:

- the manifest version is unsupported
- two enabled voices use the same ID
- required metadata is empty
- a configured `.htsvoice` file is missing
- JBonsai cannot load a configured voice

KoeServe intentionally does **not** download voice models during server startup. Model acquisition is explicit through `koeserve models fetch`, so serving never depends on an upstream model host.

## Run

```bash
cargo run
```

Default address:

```text
http://127.0.0.1:3000
```

Swagger UI:

```text
http://127.0.0.1:3000/docs
```

OpenAPI document:

```text
http://127.0.0.1:3000/openapi.json
```

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `KOESERVE_ADDR` | `127.0.0.1:3000` | HTTP listen address |
| `KOESERVE_MODELS_CONFIG` | `models/manifest.toml` | Model manifest path |
| `KOESERVE_WORKERS` | `2` | Number of CPU synthesis workers |
| `KOESERVE_QUEUE_CAPACITY` | `32` | Maximum queued synthesis jobs |

For a 2-vCPU VPS, start with `KOESERVE_WORKERS=2` and benchmark against `1`.


## Docker

The repository includes a multi-stage `Dockerfile` and `compose.yaml`. The model store is mounted as a volume instead of being baked into the image.

Build the image:

```bash
docker compose build
```

Fetch configured models into `./models` (read-write setup container):

```bash
docker compose --profile setup run --rm model-setup
```

The sample manifest currently contains sources without pinned SHA-256 values, so the setup service uses `--allow-unverified`. For production, pin each source `sha256` in `models/manifest.toml` and remove that flag from `compose.yaml`.

Start the HTTP server:

```bash
docker compose up -d koeserve
```

The serving container mounts the model store read-only:

```yaml
volumes:
  - ./models:/models:ro
```

and uses:

```text
KOESERVE_MODELS_CONFIG=/models/manifest.toml
KOESERVE_ADDR=0.0.0.0:3000
```

The default Compose port binding is `127.0.0.1:3000:3000`, so KoeServe is only reachable from the host unless you deliberately expose it through a reverse proxy or change the binding.

The resulting model store looks like:

```text
models/
├ manifest.toml
├ mei/
├ takumi/
├ tohoku/
├ m001/
└ licenses/
```

## API

### `GET /health`

Basic HTTP health check.

### `GET /v1/voices`

Returns loaded voices and their model-derived defaults, including sampling frequency, frame period, and alpha.

### `POST /v1/speech`

Example:

```bash
curl \
  -H 'content-type: application/json' \
  -d '{"text":"こんにちは","voice":"mei_normal","speed":1.0,"pitch":0.0}' \
  http://127.0.0.1:3000/v1/speech \
  --output speech.wav
```

Example request:

```json
{
  "text": "こんにちは",
  "voice": "mei_normal",
  "speed": 1.0,
  "pitch": 0.0,
  "alpha": null,
  "beta": null,
  "volume_db": 0.0
}
```

When an option is omitted, the worker resets the engine to the loaded model defaults before synthesis, preventing settings from a previous request from leaking into the next one.

## Model download CLI

Model download metadata lives in `models/manifest.toml`, alongside the model store it describes. Downloads are always explicit; starting the HTTP server never contacts a model host.

```bash
# Show voices, installation state, source and license
cargo run -- models list

# Fetch a voice (downloads its source once, then extracts every voice mapped to it)
cargo run -- models fetch tohoku_neutral --allow-unverified

# A source ID can also be specified directly
cargo run -- models fetch tohoku-f01 --allow-unverified

# Download MMDAgent Example 1.8 once and extract Mei/Takumi/SLT entries
cargo run -- models fetch mmdagent-example-1.8 --allow-unverified

# Fetch every configured downloadable source
cargo run -- models fetch --all --allow-unverified

# Check installed files. If file_sha256 is configured, it is verified.
cargo run -- models verify
```

For reproducible deployments, set a 64-character `sha256` on every `[[sources]]` entry and omit `--allow-unverified`. KoeServe verifies the downloaded archive/file before extracting anything. An optional `file_sha256` can also be set per voice to verify the installed `.htsvoice` itself.

Supported source formats:

```toml
format = "raw"
format = "zip"
format = "tar_gz"
```

Archive entries are selected with `source_path`. KoeServe rejects absolute paths and parent-directory traversal in archive paths, extracts only configured model members, writes through a temporary file, and then renames into the final model path.

Example source + voices:

```toml
[[sources]]
id = "example-pack"
url = "https://example.invalid/voices.zip"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
format = "zip"
license = "CC-BY-4.0"
license_url = "https://example.invalid/license"

[[voices]]
id = "example_happy"
speaker = "example"
style = "happy"
display_name = "Example Happy"
path = "example/happy.htsvoice"
source = "example-pack"
source_path = "voices/happy.htsvoice"
```

The sample manifest includes three upstream sources:

- **MMDAgent Example 1.8** — Mei (`normal`, `happy`, `angry`, `sad`, `bashful`), Takumi (`normal`, `happy`, `angry`, `sad`), and CMU ARCTIC SLT. One archive download is shared by all of these entries. SLT is catalogued but disabled because it is an English HTS voice and is not compatible with KoeServe's Japanese JPreprocess pipeline.
- **Tohoku-f01** — `neutral`, `happy`, `angry`, and `sad`.
- **NIT ATR503 M001** — the standard Japanese M001 voice.

MMDAgent Example 1.8 is fetched from the official SourceForge distribution. Its top-level README directs users to the README/COPYRIGHT files in each content directory; preserve the relevant attribution/license notices when redistributing the voice files.

### Retained model license files

`models fetch` keeps upstream license/notice files alongside the installed models. Archive members configured with `license_paths` are copied under:

```text
models/licenses/<source-id>/<archive-member-path>
```

For sources without bundled notice files, `license_url` is downloaded as `models/licenses/<source-id>/REMOTE_LICENSE.txt`.

`models verify` checks both model files and required retained license files. If the models were fetched by an older KoeServe version, run `models fetch <source-id> --allow-unverified` once again; KoeServe will re-download the source only when the retained license files are missing.

## Playground

KoeServe includes a small browser-based TTS playground under `playground/`.
It uses Vite + React and an OpenAPI-derived TypeScript client (`openapi-typescript` + `openapi-fetch`).

The playground can:

- load the available voices from `GET /v1/voices`
- select a voice and apply its model defaults
- edit text, speed, pitch, alpha, beta and volume
- call `POST /v1/speech`
- play the returned WAV directly in the browser
- download the generated WAV

### Development

Start KoeServe with the OpenAPI document enabled:

```bash
cargo run -- serve --openapi
```

Then in another terminal:

```bash
cd playground
npm install
npm run generate:api
npm run dev
```

The Vite dev server proxies `/v1`, `/health` and `/openapi.json` to KoeServe on `127.0.0.1:3000`.

Before a native Rust build that should embed the real playground assets, build the frontend first:

```bash
cd playground
npm install
npm run build
cd ..
cargo build --release
```

The Docker build performs the frontend build automatically and embeds `playground/dist` into the KoeServe binary.

## Serve options

Production-only API mode is the default. No API documentation or playground routes are exposed unless explicitly enabled.

```bash
# Production default: /health and /v1/* only
koeserve serve

# Expose only the OpenAPI JSON
koeserve serve --openapi

# Expose Swagger UI at /docs and OpenAPI JSON at /openapi.json
koeserve serve --docs

# Expose the embedded playground at /
koeserve serve --playground

# Local development/demo mode
koeserve serve --docs --playground
```

`--docs` implies that `/openapi.json` is also exposed.

## Docker

The default Compose service is production-oriented and starts without Swagger UI, OpenAPI JSON, or the playground:

```bash
docker compose up -d koeserve
```

An opt-in playground profile is also included. It runs on `127.0.0.1:3001` with Swagger UI and the playground enabled:

```bash
docker compose --profile playground up koeserve-playground
```

Model setup remains a separate write-enabled operation:

```bash
docker compose --profile setup run --rm model-setup
```

The normal server services mount `./models` read-only.
