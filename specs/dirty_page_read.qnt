module dirty_page_read {
  /// Model of the dirty-page read TOCTOU fix from PR #46.
  ///
  /// A page can be dirty in memory, written to staging, and then evicted from
  /// the disk cache before a read request arrives. The fix requires that such
  /// a read fail hard (return an error) rather than silently returning zeros
  /// or stale backend data.
  ///
  /// The model tracks a single abstract page with three states:
  ///   * Clean     -> read succeeds from backend
  ///   * Dirty     -> read succeeds from the in-memory/staging copy
  ///   * Evicted   -> the dirty copy is gone; read must error

  type PageState = str
  type ReadResult = str

  pure val states: Set[PageState] = Set("Clean", "Dirty", "Evicted")
  pure val results: Set[ReadResult] = Set("None", "Success", "Error")

  var pageState: PageState
  var lastReadResult: ReadResult
  /// True if a dirty write has ever happened and not yet been flushed.
  var dirtyPending: bool

  action init = all {
    pageState' = "Clean",
    lastReadResult' = "None",
    dirtyPending' = false,
  }

  /// Application writes the page; it becomes dirty.
  action write = {
    all {
      pageState' = "Dirty",
      lastReadResult' = "None",
      dirtyPending' = true,
    }
  }

  /// The page is evicted from the disk cache. A dirty page that is evicted
  /// loses its in-memory/staging copy and becomes unreadable.
  action evict = {
    all {
      pageState' = if (pageState == "Dirty") "Evicted" else pageState,
      lastReadResult' = "None",
      dirtyPending' = dirtyPending,
    }
  }

  /// A checkpoint/flush makes the dirty page clean and durable in backend.
  action flush = {
    all {
      pageState' = if (pageState == "Dirty") "Clean" else pageState,
      lastReadResult' = "None",
      dirtyPending' = false,
    }
  }

  /// Read the page using the correct behavior.
  action read = {
    val result =
      if (pageState == "Clean") "Success"
      else if (pageState == "Dirty") "Success"
      else "Error"
    all {
      pageState' = pageState,
      lastReadResult' = result,
      dirtyPending' = dirtyPending,
    }
  }

  action step = any {
    write,
    evict,
    flush,
    read,
  }

  /// A read of an evicted page must report an error, never success.
  def evictedReadErrors: bool =
    pageState == "Evicted" implies lastReadResult != "Success"

  /// A successful read result is only possible from Clean or Dirty pages.
  def successOnlyFromReadable: bool =
    lastReadResult == "Success" implies (pageState == "Clean" or pageState == "Dirty")

  /// A dirty write is never silently lost: an evicted dirty page cannot become
  /// Clean without an explicit flush.
  def dirtyWriteNotLost: bool =
    dirtyPending implies pageState != "Clean"

  def safety: bool = and {
    evictedReadErrors,
    successOnlyFromReadable,
    dirtyWriteNotLost,
  }

  /// Bad old behavior: read of an evicted dirty page succeeds by returning
  /// zeros or stale data. The action places the model directly in the violating
  /// state so the single-step negative check demonstrates the regression.
  action badReadEvictedAsSuccess = {
    all {
      pageState' = "Evicted",
      lastReadResult' = "Success",
      dirtyPending' = true,
    }
  }

  /// Bad old behavior: read of an evicted dirty page silently transitions the
  /// page to Clean, as if the missing dirty write never happened.
  action badEvictedToClean = {
    all {
      pageState' = "Clean",
      lastReadResult' = "None",
      dirtyPending' = true,
    }
  }
}
