# turbolite

SQLite for Node.js with compressed page groups, optional S3 cloud storage, and read-only HTTPS mode. Returns standard **better-sqlite3** connections with full API: prepared statements, param binding, transactions, user-defined functions, aggregates, and more.

## Install

### From source / local clone (recommended for this fork)

```bash
git clone https://github.com/pentamorfico/turbolite.git
cd turbolite/turbolite-ffi/packages/node

# Build the Rust extension with HTTPS enabled (requires cargo: https://rustup.rs)
npm run build-ext

# Patch better-sqlite3 for URI support and rebuild it
npm install

# Run tests
npm test
```

> **Why not `npm install github:pentamorfico/turbolite`?**
> npm does not support subdirectory installs from a monorepo.
> The clone-and-build flow above is the supported installation path for this fork.

### From npm (upstream)

```bash
npm install turbolite
```

## Usage

### Local mode (compressed, file-first)

```js
const turbolite = require('turbolite');

// /data/app.db is the user-visible local page image (turbolite-owned).
// /data/app.db-turbolite/ holds hidden implementation state.
const db = turbolite.connect('/data/app.db');

const insert = db.prepare('INSERT INTO users (name, age) VALUES (?, ?)');
insert.run('alice', 30);

const rows = db.prepare('SELECT * FROM users WHERE age > ?').all(20);
// [{ id: 1, name: 'alice', age: 30 }]

db.close();
```

### S3 cloud mode

```js
const db = turbolite.connect('my.db', {
  mode: 's3',
  bucket: 'my-bucket',
  endpoint: 'https://fly.storage.tigris.dev',
  region: 'auto',
});
```

### HTTPS read-only mode

Query a turbolite database published as static files on any HTTPS server.
The connection is **read-only**.

**Constraints on the remote layout:**
- The remote URL must expose `manifest.msgpack` and a `p/` directory.
- A plain `.db` file served over HTTPS is **not** a valid backend.
- A local sidecar/cache is created at `<path>-turbolite/`.

```js
const turbolite = require('turbolite');

const db = turbolite.connect('/tmp/emapper.db', {
  mode: 'https',
  baseUrl: 'https://sid.erda.dk/share_redirect/GMqhSrgpvx/emapper_turbolite_https_1m',
});
console.log(db.prepare('SELECT COUNT(*) AS n FROM sqlite_master').get());
db.close();
```

With a bearer token for authenticated endpoints:

```js
const db = turbolite.connect('/tmp/mydb.db', {
  mode: 'https',
  baseUrl: 'https://cdn.example.com/mydb',
  bearerToken: 'tok123',
});
```

## API

### `turbolite.connect(path, options?)`

Open a database. Returns a standard `better-sqlite3.Database`.

- **path** `string` -- Path to the database file.
- **options** `object` -- Optional. Defaults to local compressed mode.

#### Options

| Option | Type | Default | Description |
|---|---|---|---|
| `mode` | `'local' \| 's3' \| 'https'` | `'local'` | Storage mode. |
| `bucket` | `string` | — | S3 bucket name (required for `mode='s3'`). |
| `endpoint` | `string` | AWS S3 | Custom S3 endpoint URL. |
| `prefix` | `string` | auto | S3 key prefix. |
| `region` | `string` | SDK default | AWS region. |
| `cacheDir` | `string` | `<dbPath>-turbolite` | Local sidecar directory. |
| `compressionLevel` | `number` | `3` | Zstd compression level 1-22. |
| `readOnly` | `boolean` | `false` | Open in read-only mode. |
| `pageCache` | `string` | `'64MB'` | In-memory page cache size. Set to `'0'` to disable. |
| `baseUrl` | `string` | — | Root HTTPS URL (required for `mode='https'`). |
| `bearerToken` | `string` | — | ****** for authenticated HTTPS endpoints. |

### `turbolite.load(db)`

Load the turbolite extension into an existing better-sqlite3 Database.

### `turbolite.stateDirForDatabasePath(dbPath)`

Return the hidden sidecar directory path for a file-first database path.

## HTTPS mode — limitations

- **Read-only.** Any attempt to write raises an error.
- The remote must expose `manifest.msgpack` and `p/` at `baseUrl`.
  A plain `.db` file will not work.
- Pages are fetched on demand via HTTP Range requests.
- Only bearer-token authentication is supported.

## Build from source

```bash
cd turbolite-ffi/packages/node

# Build Rust extension with HTTPS enabled
npm run build-ext
# => produces turbolite.so (Linux) or turbolite.dylib (macOS)

# Patch + rebuild better-sqlite3 for URI support
npm install

# Run tests
npm test
```

## Environment variables

| Variable | Description |
|---|---|
| `TURBOLITE_EXT_PATH` | Override path to the loadable extension binary |
| `TURBOLITE_BUCKET` | S3 bucket name (S3 mode) |
| `TURBOLITE_REGION` | AWS region (S3 mode) |
| `TURBOLITE_ENDPOINT_URL` | Custom S3 endpoint URL (S3 mode) |
| `TURBOLITE_MEM_CACHE_BUDGET` | Page cache size (default `64MB`) |
| `TURBOLITE_COMPRESSION_LEVEL` | Zstd level 1-22 (default `3`) |
| `TURBOLITE_BASE_URL` | Root HTTPS URL (HTTPS mode, fallback for `baseUrl`) |
| `TURBOLITE_BEARER_TOKEN` | ****** for authenticated HTTPS endpoints |

