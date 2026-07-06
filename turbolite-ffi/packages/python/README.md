# turbolite

Compressed SQLite for Python. Transparent zstd compression via a custom VFS — works with the standard `sqlite3` module. Supports local, S3, and **HTTPS read-only** modes.

## Install from the fork (builds native extension from source)

Requires Rust/Cargo ≥ 1.75 and a C compiler.

```bash
pip install "git+https://github.com/pentamorfico/turbolite.git@main#subdirectory=turbolite-ffi/packages/python"
```

Or pin to a specific commit:

```bash
pip install "git+https://github.com/pentamorfico/turbolite.git@87303b612b6852a2ffbea2a12e936c5576cab9a0#subdirectory=turbolite-ffi/packages/python"
```

The setup.py automatically runs `cargo build --features loadable-extension,cli-s3,https,zstd` and bundles the resulting `.so`/`.dylib` inside the wheel.

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

### HTTPS read-only mode

Query a turbolite database published as static files on any HTTPS server. The connection is **read-only**.

- `base_url` must point to the **root of the turbolite object tree** (not a plain `.db` file).
- The remote must expose `manifest.msgpack` and a `p/` directory of page-group files.
- A local sidecar directory (`<path>-turbolite/`) is created next to the local cache file.

```python
conn = turbolite.connect(
    "/tmp/mydb.db",
    mode="https",
    base_url="https://sid.erda.dk/share_redirect/GMqhSrgpvx/emapper_turbolite_https_1m",
)
count = conn.execute("SELECT COUNT(*) FROM sqlite_master").fetchone()[0]
print(count)
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

**HTTPS is read-only.** `INSERT`, `UPDATE`, `DELETE`, `CREATE TABLE`, and any write operation will fail.

### S3 cloud mode

```python
conn = turbolite.connect("/data/app.db", mode="s3",
    bucket="my-bucket",
    endpoint="https://t3.storage.dev")
```

## API

### `turbolite.connect(path, *, mode="local", **kwargs)`

| Parameter | Type | Default | Description |
|---|---|---|---|
| `path` | `str` | required | Local path for the page image / cache file |
| `mode` | `str` | `"local"` | `"local"`, `"s3"`, or `"https"` |
| `base_url` | `str` | — | Root URL of the turbolite object tree (required for `mode="https"`) |
| `bearer_token` | `str` | — | Bearer token for authenticated HTTPS endpoints |
| `bucket` | `str` | — | S3 bucket (required for `mode="s3"`) |
| `endpoint` | `str` | AWS | Custom S3 endpoint URL |
| `prefix` | `str` | derived | S3 key prefix |
| `region` | `str` | SDK default | AWS region |
| `cache_dir` | `str` | `<path>-turbolite` | Override sidecar directory |
| `compression_level` | `int` | 3 | Zstd level 1-22 |
| `prefetch_threads` | `int` | num_cpus+1 | Prefetch worker threads |
| `read_only` | `bool` | `False` | Open in read-only mode |
| `page_cache` | `str` | `"64MB"` | In-memory page cache size |

### Environment variables

| Variable | Description |
|---|---|
| `TURBOLITE_BASE_URL` | Fallback for `base_url` (HTTPS mode) |
| `TURBOLITE_BEARER_TOKEN` | Fallback for `bearer_token` (HTTPS mode) |
| `TURBOLITE_BUCKET` | S3 bucket (S3 mode) |
| `TURBOLITE_ENDPOINT_URL` | Custom S3 endpoint |
| `TURBOLITE_REGION` | AWS region |
| `TURBOLITE_MEM_CACHE_BUDGET` | Page cache size (default `64MB`) |
| `TURBOLITE_COMPRESSION_LEVEL` | Zstd level 1-22 |

## Build from source (development)

```bash
# From the repo root
cd turbolite-ffi
CARGO_TARGET_DIR=$(pwd)/target cargo build --release --lib \
  --no-default-features \
  --features loadable-extension,cli-s3,https,zstd

# Copy the extension into the package, then install
cp target/release/libturbolite_ffi.so packages/python/turbolite/turbolite.so  # Linux
# cp target/release/libturbolite_ffi.dylib packages/python/turbolite/turbolite.dylib  # macOS

cd packages/python
pip install -e .
```
