---
default: patch
---

# Keep earlier exports tracked during maintenance

Earlier versions remain recorded so Clean preserves them and Consolidate moves
them with their asset. Exports also recreate deleted destination folders and
handle duplicate detection safely when revisions export concurrently.
