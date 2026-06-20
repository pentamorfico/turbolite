module mem_cache_epoch {
  /// Model of the crossbeam-epoch memory reclamation contract for the
  /// in-memory page cache, introduced in PR #46.
  ///
  /// The disk cache keeps immutable page buffers in AtomicPtr slots. A reader
  /// holds a read lock, loads the pointer, and copies while pinned to an epoch.
  /// An installer/evictor that swaps the pointer retires the old buffer through
  /// the epoch collector. The collector guarantees the buffer is not freed
  /// until every reader that pinned an epoch before the retirement has unpinned.
  ///
  /// This model abstracts the slot to a single page and the epoch collector to
  /// a simple "retired but not freed until no readers" rule.

  type PageState = str

  pure val states: Set[PageState] = Set("Absent", "Present", "Retired")

  /// Current logical state of the cached page.
  var pageState: PageState
  /// Number of readers that have loaded the pointer and are still copying.
  var readers: int
  /// True while the underlying allocation for a Retired page is still alive.
  var bufferAlive: bool

  action init = all {
    pageState' = "Absent",
    readers' = 0,
    bufferAlive' = false,
  }

  /// A reader starts copying a Present page. This corresponds to pinning an
  /// epoch and loading the AtomicPtr.
  action startRead = {
    all {
      pageState == "Present",
      readers' = readers + 1,
      pageState' = pageState,
      bufferAlive' = bufferAlive,
    }
  }

  /// A reader finishes copying and unpins its epoch.
  action finishRead = {
    all {
      readers > 0,
      readers' = readers - 1,
      pageState' = pageState,
      bufferAlive' = bufferAlive,
    }
  }

  /// Install a new version of the page. The new buffer is Present and alive.
  /// The old buffer (if Present or Retired) is retired through the epoch
  /// collector: it stays alive and can only be freed once all readers finish.
  action install = {
    all {
      pageState' = "Present",
      bufferAlive' = true,
      readers' = readers,
    }
  }

  /// Evict the page. A Present page is retired; an already Retired page stays
  /// retired; an Absent page stays absent.
  action evict = {
    all {
      pageState' =
        if (pageState == "Present") "Retired"
        else if (pageState == "Retired") "Retired"
        else "Absent",
      bufferAlive' =
        if (pageState == "Present" or pageState == "Retired") true else false,
      readers' = readers,
    }
  }

  /// The epoch collector reclaims a Retired buffer once no readers remain.
  action reclaim = {
    all {
      pageState == "Retired",
      readers == 0,
      pageState' = "Absent",
      bufferAlive' = false,
      readers' = readers,
    }
  }

  action step = any {
    startRead,
    finishRead,
    install,
    evict,
    reclaim,
  }

  /// If any reader is still copying, the page buffer is either Present or
  /// Retired-and-alive. It has never been freed.
  def readerSeesLiveBuffer: bool =
    readers > 0 implies (pageState == "Present" or (pageState == "Retired" and bufferAlive))

  /// A Retired page always has a live buffer until the collector reclaims it.
  def retiredBufferAlive: bool =
    pageState == "Retired" implies bufferAlive

  /// Reclamation only happens when no readers remain.
  def reclaimOnlyWhenIdle: bool =
    not(bufferAlive) implies readers == 0

  def safety: bool = and {
    readerSeesLiveBuffer,
    retiredBufferAlive,
    reclaimOnlyWhenIdle,
  }

  /// Bad old behavior: the 64-generation heuristic frees a Retired buffer even
  /// though a reader is still copying. This is the use-after-free the epoch
  /// collector prevents. The action places the model directly in the violating
  /// state so the single-step negative check demonstrates the regression.
  action badFreeWithActiveReaders = {
    all {
      pageState' = "Absent",
      bufferAlive' = false,
      readers' = 1,
    }
  }

  /// Bad old behavior: a reader is allowed to start after the buffer has been
  /// freed. In the real code the read lock prevents this, but the model
  /// captures the underlying UAF if it were possible.
  action badReadAfterFree = {
    all {
      pageState == "Absent",
      bufferAlive' = false,
      readers' = readers + 1,
      pageState' = pageState,
    }
  }
}
