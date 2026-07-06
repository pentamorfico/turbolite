# turbolite

SQLite for Node.js with compressed page groups and optional S3 cloud storage. Returns standard **better-sqlite3** connections with full API: prepared statements, param binding, transactions, user-defined functions, aggregates, and more.

## Install from the fork (builds native extension from source)

Requires Rust/Cargo ≥ 1.75 and a C compiler. `npm install turbolite` (npm registry) will not work for this fork. Instead, clone and install locally:

```bash
git clone https://github.com/pentamorfico/turbolite.git
cd turbolite/turbolite-ffi/packages/node

# 1. Build the native loadable extension (features: loadable-extension,cli-s3,https,zstd)
npm run build-ext

# 2. Install dependencies (patches better-sqlite3 for SQLITE_USE_URI=1 and rebuilds it)
npm install

# 3. Build TypeScript
npm run build
```

Then in your project, use a `file:` path or `npm pack` + install the tarball:

```bash
# From turbolite-ffi/packages/node:
npm pack
# Produces turbolite-0.5.0.tgz — install in your project:
npm install /path/to/turbolite-0.5.0.tgz
```

> **Why not `npm install` from a Git URL?**
> npm Git installs run in a sandbox without the parent `turbolite-ffi/` Cargo workspace, so `build-ext` cannot find the Rust sources. Clone the repo and install locally (as above) or from a packed tarball.

## Usage

### Local mode (compressed, file-first)

```js
const turbolite = require('turbolite');

// /data/app.db is the user-visible local page image (turbolite-owned).
// /data/app.db-turbolite/ holds hidden implementation state
// (manifest, cache, staging logs).
const db = turbolite.connect('/data/app.db');

// Full better-sqlite3 API: prepared statements, param binding, transactions
const insert = db.prepare('INSERT INTO users (name, age) VALUES (?, ?)');
insert.run('alice', 30);
insert.run('bob', 25);

const rows = db.prepare('SELECT * FROM users WHERE age > ?').all(20);
// [{ id: 1, name: 'alice', age: 30 }, { id: 2, name: 'bob', age: 25 }]

// Transactions
const batchInsert = db.transaction((users) => {
  for (const u of users) insert.run(u.name, u.age);
});
batchInsert([{ name: 'charlie', age: 35 }]);

db.close();
```

`app.db` is turbolite's compressed page image. It is not promised to be
opened directly by stock SQLite. To produce a normal SQLite file the
`sqlite3` CLI can read, use better-sqlite3's online backup API:

```js
await db.backup('/data/exported.sqlite');
```

(`db.backup` opens the destination via SQLite's default VFS, sidestepping
the file-first VFS's identity check on alias opens.)

### S3 cloud mode

Pages are compressed locally and durably synced to S3 (or any S3-compatible store).

```js
const db = turbolite.connect('my.db', {
  mode: 's3',
  bucket: 'my-bucket',
  region: 'us-east-1',
});
```

With a custom S3-compatible endpoint (Tigris, MinIO, etc.):

```js
const db = turbolite.connect('my.db', {
  mode: 's3',
  bucket: 'my-bucket',
  endpoint: 'https://fly.storage.tigris.dev',
  region: 'auto',
});
```

### HTTPS read-only mode

Query a turbolite database published as static files on any HTTPS server (CDN, S3 static website, etc.). The connection is **read-only**.

- `baseUrl` must point to the **root of the turbolite object tree** — not a plain `.db` file.
- The remote must expose `manifest.msgpack` and a `p/` directory of compressed page-group files.
- A local sidecar directory (`<path>-turbolite/`) caches fetched pages next to the local file path.
- **HTTPS is read-only.** Any write operation will fail.

```js
const { connect } = require('turbolite');

const db = connect('/tmp/mydb.db', {
  mode: 'https',
  baseUrl: 'https://example.com/turbolite/mydb',
});

const count = db.prepare('SELECT COUNT(*) FROM sqlite_master').pluck().get();
console.log(count);
db.close();
```

With a bearer token for authenticated endpoints:

```js
const db = connect('/tmp/mydb.db', {
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
| `bucket` | `string` | -- | S3 bucket name (required for `mode='s3'`). |
| `endpoint` | `string` | AWS S3 | Custom S3 endpoint URL. |
| `prefix` | `string` | `'turbolite'` | S3 key prefix. |
| `region` | `string` | SDK default | AWS region. |
| `cacheDir` | `string` | -- | Lower-level: override the sidecar location (default is `<dbPath>-turbolite`). Mostly useful in S3 mode. |
| `compressionLevel` | `number` | `3` | Zstd compression level 1-22. |
| `readOnly` | `boolean` | `false` | Open in read-only mode. |
| `pageCache` | `string` | `'64MB'` | In-memory page cache size. Set to `'0'` to disable. |
| `baseUrl` | `string` | -- | Root URL of the turbolite object tree (required for `mode='https'`). |
| `bearerToken` | `string` | -- | Bearer token for authenticated HTTPS endpoints (`mode='https'`). |

### `turbolite.load(db)`

Load the turbolite extension into an existing better-sqlite3 Database.

### `turbolite.stateDirForDatabasePath(dbPath)`

Return the hidden sidecar directory path for a file-first database path.
For `/data/app.db` this is `/data/app.db-turbolite`. Useful for tests or
tooling that asserts on layout without hardcoding the suffix rule.

## Architecture

Each `connect()` call creates an isolated VFS instance keyed to the
caller's database path. The user-supplied path is the local page image;
hidden implementation state lives at `<dbPath>-turbolite/`.

Under the hood:
1. The turbolite loadable extension is loaded into better-sqlite3's SQLite
2. `turbolite_register_file_first_vfs(name, dbPath)` creates a named VFS
   instance bound to that path
3. The database is opened via URI: `file:dbPath?vfs=turbolite-node-N`
4. `PRAGMA cache_size=0` disables SQLite's cache (turbolite manages its own)

The lower-level `turbolite_register_vfs(name, cacheDir)` SQL function is
still available for embedders that want turbolite to own a directory and
store the local image at `<cacheDir>/data.cache`. New code should prefer
the file-first form.

## Build from source

```bash
cd packages/node

# Build the loadable extension (requires Rust)
npm run build-ext

# Install dependencies (patches + rebuilds better-sqlite3 for URI support)
npm install

# Run tests
npm test
```

## Environment variables

| Variable | Description |
|---|---|
| `TURBOLITE_EXT_PATH` | Override path to the loadable extension binary |
| `TURBOLITE_DATABASE_PATH` | File-first database path for the default `"turbolite"` VFS (extension-load time) |
| `TURBOLITE_BUCKET` | S3 bucket name (S3 mode) |
| `TURBOLITE_REGION` | AWS region (S3 mode) |
| `TURBOLITE_ENDPOINT_URL` | Custom S3 endpoint URL (S3 mode) |
| `TURBOLITE_MEM_CACHE_BUDGET` | Page cache size (default `64MB`) |
| `TURBOLITE_COMPRESSION_LEVEL` | Zstd level 1-22 (default `3`) |
| `TURBOLITE_BASE_URL` | Root HTTPS URL (HTTPS mode, fallback for `baseUrl`) |
| `TURBOLITE_BEARER_TOKEN` | Bearer token for authenticated HTTPS endpoints (HTTPS mode) |
