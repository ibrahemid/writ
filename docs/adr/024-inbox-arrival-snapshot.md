# ADR-024: Snapshot-Based Inbox Arrival Detection

**Status:** Accepted
**Date:** 2026-07-05
**Revises:** ADR-018 (the created-after-watch-start rule)

## Context

ADR-018 discriminated new inbox arrivals from pre-existing backlog by comparing the file's creation timestamp (birth time, falling back to mtime) against the instant the watcher started. Filesystem timestamp granularity breaks this: on filesystems that store birth time coarsely, a file created moments after watch start can carry a stored timestamp that rounds below the watcher's start instant and is silently suppressed. The failure is real for users (a file dropped into the inbox within the same clock tick as enabling it never opens) and made the arrival integration test time-dependent.

## Decision

Membership, not time. When the inbox watcher starts, it snapshots the set of file paths already present under the root (recursively), immediately after registering the filesystem watch. An event qualifies as an arrival if and only if its path is absent from that snapshot, in addition to the unchanged containment and ignore-set rules.

- `writ_core::inbox::qualifies_for_auto_open(root, path, preexisting)` owns the policy; the adapter supplies the snapshot.
- The snapshot is taken after watch registration, so a file landing during the scan is suppressed as pre-existing rather than reported twice, and nothing created after the scan can be missed.
- Pre-existing files that are later modified stay suppressed for the lifetime of the watch, exactly as before.

## Consequences

- Arrival detection no longer depends on filesystem timestamp support or granularity; behavior is identical across APFS, ext4, tmpfs, and NTFS.
- The integration tests are deterministic: a file written after `set_inbox_path` returns is always an arrival, a file written before is never one.
- A new file that is deleted and recreated at the same path while watched still qualifies (it is absent from the snapshot); under the timestamp rule the same was true, so no behavior change.
- The snapshot holds one `PathBuf` per pre-existing file for the lifetime of the watch. Inbox folders are working directories, not archives; the cost is negligible.
