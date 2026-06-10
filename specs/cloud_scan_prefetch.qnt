module cloud_scan_prefetch {
  type Group = int

  pure val groups: Set[Group] = Set(0, 1, 2, 3)
  /// Planned SCAN ownership is group-membership based. The Rust read path must
  /// not require a successfully inferred current tree name before treating the
  /// current group as part of the planned scan.
  pure val plannedGroups: Set[Group] = groups
  pure val windowGroups: int = 2

  var current: Group
  var queued: Set[Group]
  var permitBlocked: Set[Group]
  var fetching: Set[Group]
  var present: Set[Group]
  var startedRemote: Set[Group]
  var foregroundRange: Set[Group]
  var foregroundFull: Set[Group]
  var foregroundWaited: Set[Group]
  var prefetchFull: Set[Group]

  action init = all {
    current' = 0,
    queued' = Set(),
    permitBlocked' = Set(),
    fetching' = Set(),
    present' = Set(),
    startedRemote' = Set(),
    foregroundRange' = Set(),
    foregroundFull' = Set(),
    foregroundWaited' = Set(),
    prefetchFull' = Set(),
  }

  action noStateChange = all {
    current' = current,
    queued' = queued,
    permitBlocked' = permitBlocked,
    fetching' = fetching,
    present' = present,
    startedRemote' = startedRemote,
    foregroundRange' = foregroundRange,
    foregroundFull' = foregroundFull,
    foregroundWaited' = foregroundWaited,
    prefetchFull' = prefetchFull,
  }

  def inAheadWindow(gid: Group): bool =
    gid > current and gid <= current + windowGroups

  def claimFree(gid: Group): bool =
    not(queued.contains(gid))
      and not(permitBlocked.contains(gid))
      and not(fetching.contains(gid))
      and not(present.contains(gid))

  def liveOptional: Set[Group] =
    queued.union(permitBlocked).union(fetching)

  action submitOptionalAhead = {
    nondet gid = oneOf(groups.filter(gid =>
      plannedGroups.contains(gid)
        and liveOptional.size() < windowGroups
        and inAheadWindow(gid)
        and claimFree(gid)
    ))
    all {
      current' = current,
      queued' = queued.union(Set(gid)),
      permitBlocked' = permitBlocked,
      fetching' = fetching,
      present' = present,
      startedRemote' = startedRemote,
      foregroundRange' = foregroundRange,
      foregroundFull' = foregroundFull,
      foregroundWaited' = foregroundWaited,
      prefetchFull' = prefetchFull,
    }
  }

  /// Worker owns the claim only after an optional permit is available and
  /// remote I/O is starting. Queue-only work is not foreground-visible
  /// `Fetching`.
  action startOptionalRemote = {
    nondet gid = oneOf(queued)
    all {
      current' = current,
      queued' = queued.exclude(Set(gid)),
      permitBlocked' = permitBlocked,
      fetching' = fetching.union(Set(gid)),
      present' = present,
      startedRemote' = startedRemote.union(Set(gid)),
      foregroundRange' = foregroundRange,
      foregroundFull' = foregroundFull,
      foregroundWaited' = foregroundWaited,
      prefetchFull' = prefetchFull.union(Set(gid)),
    }
  }

  action completeOptional = {
    nondet gid = oneOf(fetching)
    all {
      current' = current,
      queued' = queued,
      permitBlocked' = permitBlocked,
      fetching' = fetching.exclude(Set(gid)),
      present' = present.union(Set(gid)),
      startedRemote' = startedRemote,
      foregroundRange' = foregroundRange,
      foregroundFull' = foregroundFull,
      foregroundWaited' = foregroundWaited,
      prefetchFull' = prefetchFull,
    }
  }

  action dropQueuedOptional = {
    nondet gid = oneOf(queued)
    all {
      current' = current,
      queued' = queued.exclude(Set(gid)),
      permitBlocked' = permitBlocked,
      fetching' = fetching,
      present' = present,
      startedRemote' = startedRemote,
      foregroundRange' = foregroundRange,
      foregroundFull' = foregroundFull,
      foregroundWaited' = foregroundWaited,
      prefetchFull' = prefetchFull,
    }
  }

  action foregroundReadPresent = all {
    present.contains(current),
    current' = if (current < 3) current + 1 else current,
    queued' = queued,
    permitBlocked' = permitBlocked,
    fetching' = fetching,
    present' = present,
    startedRemote' = startedRemote,
    foregroundRange' = foregroundRange,
    foregroundFull' = foregroundFull,
    foregroundWaited' = foregroundWaited,
    prefetchFull' = prefetchFull,
  }

  /// Planned cold scan demand uses full-group demand when its current group
  /// is not already present or actively fetching. It must not use seekable
  /// range demand for a group in the planned scan.
  action foregroundFullDemand = all {
    plannedGroups.contains(current),
    claimFree(current),
    current' = if (current < 3) current + 1 else current,
    queued' = queued,
    permitBlocked' = permitBlocked,
    fetching' = fetching,
    present' = present.union(Set(current)),
    startedRemote' = startedRemote,
    foregroundRange' = foregroundRange,
    foregroundFull' = foregroundFull.union(Set(current)),
    foregroundWaited' = foregroundWaited,
    prefetchFull' = prefetchFull,
  }

  /// Foreground may wait only for optional work that has already started
  /// remote I/O. It never waits behind queue-only optional work.
  action foregroundWaitStartedPrefetch = all {
    fetching.contains(current),
    startedRemote.contains(current),
    current' = if (current < 3) current + 1 else current,
    queued' = queued,
    permitBlocked' = permitBlocked,
    fetching' = fetching.exclude(Set(current)),
    present' = present.union(Set(current)),
    startedRemote' = startedRemote,
    foregroundRange' = foregroundRange,
    foregroundFull' = foregroundFull,
    foregroundWaited' = foregroundWaited.union(Set(current)),
    prefetchFull' = prefetchFull,
  }

  action advanceAfterDroppedCurrent = all {
    queued.contains(current),
    current' = current,
    queued' = queued.exclude(Set(current)),
    permitBlocked' = permitBlocked,
    fetching' = fetching,
    present' = present,
    startedRemote' = startedRemote,
    foregroundRange' = foregroundRange,
    foregroundFull' = foregroundFull,
    foregroundWaited' = foregroundWaited,
    prefetchFull' = prefetchFull,
  }

  action step = any {
    submitOptionalAhead,
    startOptionalRemote,
    completeOptional,
    dropQueuedOptional,
    foregroundReadPresent,
    foregroundFullDemand,
    foregroundWaitStartedPrefetch,
    advanceAfterDroppedCurrent,
    noStateChange,
  }

  pure def disjointCacheLifecycle: bool =
    groups.forall(gid =>
      not(queued.contains(gid) and fetching.contains(gid))
        and not(queued.contains(gid) and present.contains(gid))
        and not(queued.contains(gid) and permitBlocked.contains(gid))
        and not(permitBlocked.contains(gid) and fetching.contains(gid))
        and not(permitBlocked.contains(gid) and present.contains(gid))
        and not(fetching.contains(gid) and present.contains(gid))
    )

  pure def noOptionalBlockedOnPermit: bool =
    permitBlocked == Set()

  pure def fetchingMeansStartedRemote: bool =
    fetching.forall(gid => startedRemote.contains(gid))

  pure def queuedIsNotForegroundWaitable: bool =
    foregroundWaited.forall(gid => startedRemote.contains(gid))

  pure def noScanRangeDemand: bool =
    foregroundRange == Set()

  /// Scoped to planned cold-scan groups (this module models plannedGroups
  /// only). Non-planned point/search demand intentionally pairs one frame
  /// range GET with one optional warm full-group prefetch for the current
  /// group; that optional work goes through the same nonblocking admission
  /// (queue -> permit -> claim) modeled here, so it never re-enables the
  /// forbidden permitBlocked / claim-before-permit lifecycles.
  pure def noRangeAndFullForSameGroup: bool =
    groups.forall(gid =>
      not(foregroundRange.contains(gid) and prefetchFull.contains(gid))
    )

  pure def boundedLiveOptional: bool =
    liveOptional.size() <= windowGroups

  def safety: bool = and {
    disjointCacheLifecycle,
    noOptionalBlockedOnPermit,
    fetchingMeansStartedRemote,
    queuedIsNotForegroundWaitable,
    noScanRangeDemand,
    noRangeAndFullForSameGroup,
    boundedLiveOptional,
  }

  /// Bad old lifecycle: queued optional work claims `Fetching` before it has a
  /// permit or started remote I/O.
  action badClaimBeforePermit = {
    nondet gid = oneOf(groups.filter(gid => claimFree(gid)))
    all {
      current' = current,
      queued' = queued,
      permitBlocked' = permitBlocked,
      fetching' = fetching.union(Set(gid)),
      present' = present,
      startedRemote' = startedRemote,
      foregroundRange' = foregroundRange,
      foregroundFull' = foregroundFull,
      foregroundWaited' = foregroundWaited,
      prefetchFull' = prefetchFull,
    }
  }

  /// Bad old worker lifecycle: optional work leaves the queue but then parks
  /// waiting for a prefetch permit instead of dropping immediately.
  action badWorkerBlocksBeforePermit = {
    nondet gid = oneOf(groups.filter(gid => claimFree(gid)))
    all {
      current' = current,
      queued' = queued,
      permitBlocked' = permitBlocked.union(Set(gid)),
      fetching' = fetching,
      present' = present,
      startedRemote' = startedRemote,
      foregroundRange' = foregroundRange,
      foregroundFull' = foregroundFull,
      foregroundWaited' = foregroundWaited,
      prefetchFull' = prefetchFull,
    }
  }

  /// Bad old cold-scan policy: current planned group uses a foreground range
  /// while optional full-group prefetch also exists for that group.
  action badRangeAndFullDuplicate = all {
    claimFree(current),
    current' = current,
    queued' = queued,
    permitBlocked' = permitBlocked,
    fetching' = fetching,
    present' = present,
    startedRemote' = startedRemote,
    foregroundRange' = foregroundRange.union(Set(current)),
    foregroundFull' = foregroundFull,
    foregroundWaited' = foregroundWaited,
    prefetchFull' = prefetchFull.union(Set(current)),
  }

  /// Bad tree-name-gated scan policy: the current group belongs to the planned
  /// scan, but a missing current-tree inference sends demand down the seekable
  /// range path instead of the planned full-group path.
  action badTreeNameGatedScanRange = all {
    plannedGroups.contains(current),
    claimFree(current),
    current' = current,
    queued' = queued,
    permitBlocked' = permitBlocked,
    fetching' = fetching,
    present' = present,
    startedRemote' = startedRemote,
    foregroundRange' = foregroundRange.union(Set(current)),
    foregroundFull' = foregroundFull,
    foregroundWaited' = foregroundWaited,
    prefetchFull' = prefetchFull,
  }
}
