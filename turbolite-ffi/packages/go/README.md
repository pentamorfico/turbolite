# turbolite — Go binding

SQLite with compressed page groups and optional S3/HTTPS cloud storage for Go. Returns a standard `*sql.DB`.

## Supported consumption flow (fork)

The Go module path is `github.com/pentamorfico/turbolite/packages/go`.

### Direct `go get` (recommended)

```bash
go get github.com/pentamorfico/turbolite/packages/go@main
```

You also need to set `CGO_LDFLAGS` so cgo can find the turbolite loadable extension at link time. Build the extension first (see [Build the extension](#build-the-extension)).

### Replace-directive (if you have a local clone)

In your `go.mod`:

```
require github.com/pentamorfico/turbolite/packages/go v0.0.0
replace github.com/pentamorfico/turbolite/packages/go => /path/to/turbolite-repo/turbolite-ffi/packages/go
```

## Build the extension

The Go binding does **not** link statically against turbolite. It uses the loadable-extension `.so`/`.dylib` at runtime (dlopened by go-sqlite3). Build it once:

```bash
# From the repo root
cd turbolite-ffi
CARGO_TARGET_DIR=$(pwd)/target cargo build --release --lib \
  --no-default-features \
  --features loadable-extension,cli-s3,https,zstd

# Rename to turbolite.so/.dylib — go-sqlite3 derives the extension entry point
# from the filename; it must be "turbolite" to match sqlite3_turbolite_init.
cp target/release/libturbolite_ffi.so  target/release/turbolite.so   # Linux
# cp target/release/libturbolite_ffi.dylib target/release/turbolite.dylib  # macOS
```

Then point the binding at it with `TURBOLITE_EXT_PATH`:

```bash
export TURBOLITE_EXT_PATH=/path/to/turbolite-ffi/target/release/turbolite.so   # Linux
# export TURBOLITE_EXT_PATH=/path/to/turbolite-ffi/target/release/turbolite.dylib  # macOS
```

> **Important:** Use `turbolite.so` / `turbolite.dylib`, not `libturbolite_ffi.so`.
> go-sqlite3 derives the extension entry point from the filename: `turbolite.so` → `sqlite3_turbolite_init` ✓.
> `libturbolite_ffi.so` → `sqlite3_turbolite_ffi_init` ✗ (symbol not exported).

## Usage

```go
import turbolite "github.com/pentamorfico/turbolite/packages/go"
```

### Local mode

```go
db, err := turbolite.Open("/data/app.db", nil)
if err != nil {
    log.Fatal(err)
}
defer db.Close()

db.Exec("CREATE TABLE t (id INTEGER PRIMARY KEY, val TEXT)")
db.Exec("INSERT INTO t VALUES (1, 'hello')")

var v string
db.QueryRow("SELECT val FROM t WHERE id = 1").Scan(&v)
fmt.Println(v)  // hello
```

### HTTPS read-only mode

- `BaseURL` must point to the **root of the turbolite object tree** — not a plain `.db` file.
- The remote must expose `manifest.msgpack` and a `p/` directory of compressed page-group files.
- A local sidecar directory (`<path>-turbolite/`) caches fetched pages.
- **HTTPS is read-only.** Any write operation will fail.

```go
db, err := turbolite.Open("/tmp/mydb.db", &turbolite.Options{
    Mode:    "https",
    BaseURL: "https://example.com/turbolite/mydb",
})
if err != nil {
    log.Fatal(err)
}
defer db.Close()

var count int
db.QueryRow("SELECT COUNT(*) FROM sqlite_master").Scan(&count)
fmt.Println(count)
```

With a bearer token for authenticated endpoints:

```go
db, err := turbolite.Open("/tmp/mydb.db", &turbolite.Options{
    Mode:        "https",
    BaseURL:     "https://cdn.example.com/mydb",
    BearerToken: "tok123",
})
```

### S3 mode

```go
db, err := turbolite.Open("/data/app.db", &turbolite.Options{
    Mode:     "s3",
    Bucket:   "my-bucket",
    Endpoint: "https://t3.storage.dev",
})
```

## Environment variables

| Variable | Description |
|---|---|
| `TURBOLITE_EXT_PATH` | **Required** — full path to `turbolite.so`/`turbolite.dylib` (see [Build the extension](#build-the-extension)) |
| `TURBOLITE_BASE_URL` | Fallback for `BaseURL` (HTTPS mode) |
| `TURBOLITE_BEARER_TOKEN` | Bearer token fallback for `BearerToken` (HTTPS mode) |
| `TURBOLITE_BUCKET` | S3 bucket (S3 mode) |
| `TURBOLITE_ENDPOINT_URL` | Custom S3 endpoint |
| `TURBOLITE_REGION` | AWS region |
| `TURBOLITE_MEM_CACHE_BUDGET` | Page cache size (default `64MB`) |
| `TURBOLITE_COMPRESSION_LEVEL` | Zstd level 1-22 |

## Complete example (HTTPS)

```bash
# 1. Clone the repo and build the extension
git clone https://github.com/pentamorfico/turbolite.git
cd turbolite/turbolite-ffi
CARGO_TARGET_DIR=$(pwd)/target cargo build --release --lib \
  --no-default-features --features loadable-extension,cli-s3,https,zstd
# Rename so go-sqlite3 finds the correct entry point sqlite3_turbolite_init
cp target/release/libturbolite_ffi.so  target/release/turbolite.so   # Linux
# cp target/release/libturbolite_ffi.dylib target/release/turbolite.dylib  # macOS

export TURBOLITE_EXT_PATH=$(pwd)/target/release/turbolite.so    # Linux
# export TURBOLITE_EXT_PATH=$(pwd)/target/release/turbolite.dylib  # macOS

# 2. In your Go project
go get github.com/pentamorfico/turbolite/packages/go@main
```

```go
package main

import (
    "database/sql"
    "fmt"
    "log"

    turbolite "github.com/pentamorfico/turbolite/packages/go"
)

func main() {
    db, err := turbolite.Open("/tmp/mydb.db", &turbolite.Options{
        Mode:    "https",
        BaseURL: "https://example.com/turbolite/mydb",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer db.Close()

    rows, err := db.Query("SELECT name FROM sqlite_master LIMIT 5")
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()
    for rows.Next() {
        var name sql.NullString
        rows.Scan(&name)
        fmt.Println(name.String)
    }
}
```

## Limitations

- The extension binary must be present at runtime (no static linking). Always set `TURBOLITE_EXT_PATH`.
- HTTPS mode is read-only by design — the VFS fetches pages over HTTP range requests.
- The remote HTTPS backend must be a turbolite object tree (manifest + page groups), not a plain SQLite `.db` file.
- No CGO-free build: this binding requires CGO (`go-sqlite3`).
