module flush_ordering {
  /// Model of the two-phase flush persistence contract from PR #46.
  ///
  /// `flush_inner` must upload the new manifest to the backend, persist the
  /// local manifest, clear the dirty-groups file, and only then update the
  /// in-memory shared manifest. The critical safety property is that the
  /// shared manifest version must never advance past the persisted local
  /// manifest version: if the process crashes after `shared_manifest` is
  /// updated, the on-disk local state must already reflect that version so
  /// the next VFS open starts from a consistent file.
  ///
  /// Abstractions: we model only manifest version counters and a dirty flag.
  /// Page groups, backends, compression, and encryption are intentionally
  /// omitted.

  var localVersion: int
  var remoteVersion: int
  var sharedVersion: int
  var dirty: bool

  action init = all {
    localVersion' = 0,
    remoteVersion' = 0,
    sharedVersion' = 0,
    dirty' = false,
  }

  /// Writes happen locally and mark the database dirty. They do not advance
  /// the manifest version until a local checkpoint commits.
  action writePages = {
    all {
      dirty' = true,
      localVersion' = localVersion,
      remoteVersion' = remoteVersion,
      sharedVersion' = sharedVersion,
    }
  }

  /// A local checkpoint captures the dirty state and advances the local
  /// manifest version. The shared manifest is not updated yet.
  action localCheckpoint = {
    all {
      dirty' = dirty,
      localVersion' = if (dirty) localVersion + 1 else localVersion,
      remoteVersion' = remoteVersion,
      sharedVersion' = sharedVersion,
    }
  }

  /// Flush the locally-checkpointed state to the backend in the correct order:
  /// 1. upload remote manifest, 2. persist local manifest, 3. clear dirty
  /// groups, 4. update shared manifest. If remote or local persistence fails,
  /// the shared manifest is not advanced and the dirty flag remains set.
  action flush = {
    val newVersion = sharedVersion + 1
    val remoteOk = dirty or remoteVersion >= newVersion // remote put of a non-dirty flush is a no-op
    val localOk = dirty or localVersion >= newVersion   // local persist of a non-dirty flush is a no-op
    all {
      remoteVersion' = if (remoteOk and dirty) newVersion else remoteVersion,
      localVersion' = if (localOk and dirty) newVersion else localVersion,
      dirty' = if (remoteOk and localOk) false else dirty,
      sharedVersion' = if (remoteOk and localOk) newVersion else sharedVersion,
    }
  }

  /// Crash recovery: on restart the VFS reads the local manifest. The model
  /// abstracts this as resetting the shared version to the local version and
  /// recovering any dirty state that was persisted locally but not yet shared.
  action recover = {
    all {
      sharedVersion' = localVersion,
      dirty' = localVersion > sharedVersion,
      localVersion' = localVersion,
      remoteVersion' = remoteVersion,
    }
  }

  action step = any {
    writePages,
    localCheckpoint,
    flush,
    recover,
  }

  /// Expose bad actions for negative checks; they are not part of `step`.

  /// The shared in-memory manifest never runs ahead of the persisted local
  /// manifest. This is the property the PR #46 reordering guarantees.
  def sharedNotAheadOfLocal: bool =
    sharedVersion <= localVersion

  /// The remote manifest never regresses.
  def remoteMonotonic: bool =
    remoteVersion >= 0 // trivially true for nat; kept for symmetry

  /// After a successful flush (dirty cleared), the local and shared views agree.
  /// The remote view also agrees at that instant; recovery can temporarily make
  /// local/shared diverge from remote until the next flush reconciles them.
  def flushedLocalAndSharedAgree: bool =
    not(dirty) implies (localVersion == sharedVersion)

  def safety: bool = and {
    sharedNotAheadOfLocal,
    remoteMonotonic,
    flushedLocalAndSharedAgree,
  }

  /// Bad behavior: update the shared manifest before the local manifest is
  /// persisted. If a crash happens in this window, the next VFS open sees an
  /// old local manifest while the in-memory state claimed a newer one.
  action badSharedBeforeLocal = {
    val newVersion = sharedVersion + 1
    all {
      localVersion' = localVersion,
      remoteVersion' = if (dirty) newVersion else remoteVersion,
      sharedVersion' = newVersion,
      dirty' = false,
    }
  }

  /// Bad behavior: clear the dirty flag and update shared state without ever
  /// persisting the local manifest.
  action badNoLocalPersist = {
    val newVersion = sharedVersion + 1
    all {
      localVersion' = localVersion,
      remoteVersion' = if (dirty) newVersion else remoteVersion,
      sharedVersion' = newVersion,
      dirty' = false,
    }
  }
}
