# turbolite

Compressed SQLite for Python. Transparent zstd compression via a custom VFS — works with the standard `sqlite3` module. Supports local storage, S3 cloud storage, and read-only HTTPS (CDN/static-file) mode.

## Install

### From this fork (builds the native extension automatically)

```bash
pip install "git+https://github.com/pentamorfico/turbolite.git@main#subdirectory=turbolite-ffi/packages/python"
```

**Requirements:** `cargo` must be on `PATH`. Install from <https://rustup.rs> if missing. The build compiles the Rust extension with `loadable-extension,cli-s3,https,zstd` features.

### From PyPI (upstream)

```bash
pip install turbolite
```

## Usage

### Local mode (compressed, file-first)

```python
import turbolite

# /data/app.db is the local page image (turbolite-owned).
# /data/app.db-turbolite/ holds hidden implementation state
# (manifest, cache, staging logs).
conn = turbolite.connect("/data/app.db")
conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)")
conn.execute("INSERT INTO users VALUES (1, 'alice')")
conn.commit()

rows = conn.execute("SELECT * FROM users").fetchall()
print(rows)  # [(1, 'alice')]
conn.close()
```

### S3 cloud mode

```python
conn = turbolite.connect("/data/app.db", mode="s3",
    bucket="my-bucket",
    endpoint="https://t3.storage.dev")
```

### HTTPS read-only mode

Query a turbolite database published as static files on any HTTPS server
(CDN, public S3 bucket, university data portal, etc.). The connection is
**read-only**.

**Constraints on the remote layout:**
- The remote URL must expose `manifest.msgpack` and a `p/` directory
  with page-group objects.
- A plain `.db` file served over HTTPS is **not** a valid backend.
- A local sidecar/cache directory is created at `<path>-turbolite/`.

```python
import turbolite

# Open against a public HTTPS turbolite dataset
conn = turbolite.connect(
    "/tmp/emapper.db",
    mode="https",
    base_url="https://sid.erda.dk/share_redirect/GMqhSrgpvx/emapper_turbolite_https_1m",
)
print(conn.execute("SELECT COUNT(*) FROM sqlite_master").fetchone())
conn.close()
```

With a bearer token for authenticated endpoints:

```python
conn = turbolite.connect(
    "/tmp/mydb.db",
    mode="https",
    base_url="https://cdn.example.com/mydb",
    bearer_token="tok123",
)
```

## API

### `turbolite.connect(path, **options)`

Open a turbolite database. Returns a `sqlite3.Connection`.

| Option | Type | Default | Description |
|---|---|---|---|
| `mode` | `str` | `"local"` | `"local"`, `"s3"`, or `"https"` |
| `bucket` | `str` | — | S3 bucket (required for `mode="s3"`) |
| `endpoint` | `str` | AWS S3 | Custom S3 endpoint URL |
| `prefix` | `str` | auto | S3 key prefix |
| `region` | `str` | SDK default | AWS region |
| `cache_dir` | `str` | `<path>-turbolite` | Local sidecar directory |
| `compression_level` | `int` | `3` | Zstd level 1-22 |
| `read_only` | `bool` | `False` | Read-only mode |
| `page_cache` | `str` | `"64MB"` | Page cache size. Set to `"0"` to disable |
| `base_url` | `str` | — | Root HTTPS URL (required for `mode="https"`) |
| `bearer_token` | `str` | — | ****** for authenticated HTTPS |

### `turbolite.load(conn)`

Load the extension into an existing `sqlite3.Connection`.

### `turbolite.state_dir_for_database_path(path)`

Return the sidecar directory path (e.g. `/data/app.db-turbolite`).

## HTTPS mode — limitations

- **Read-only.** Writes raise an error.
- The remote must expose `manifest.msgpack` and `p/` at `base_url`.
  A plain `.db` file will not work.
- The first query fetches the manifest over HTTPS; subsequent pages
  are fetched on demand. A local sidecar at `<path>-turbolite/` caches
  downloaded pages.
- No authentication beyond a bearer token is supported.

## Build from source

```bash
# From the turbolite-ffi/ directory
cd turbolite-ffi

# Build extension with HTTPS enabled
make ext EXT_FEATURES="cli-s3,https,zstd"

# Copy and install in dev mode
cp ../../target/release/libturbolite_ffi.so packages/python/turbolite/turbolite.so  # Linux
# cp ../../target/release/libturbolite_ffi.dylib packages/python/turbolite/turbolite.dylib  # macOS
cd packages/python && pip install -e .
```

## Environment variables

| Variable | Description |
|---|---|
| `TURBOLITE_BUCKET` | S3 bucket (S3 mode) |
| `TURBOLITE_ENDPOINT_URL` | Custom S3 endpoint (S3 mode) |
| `TURBOLITE_BASE_URL` | Root HTTPS URL (HTTPS mode fallback) |
| `TURBOLITE_BEARER_TOKEN` | ****** for authenticated HTTPS |
| `TURBOLITE_MEM_CACHE_BUDGET` | Page cache size (default `64MB`) |
| `TURBOLITE_COMPRESSION_LEVEL` | Zstd level 1-22 (default `3`) |

