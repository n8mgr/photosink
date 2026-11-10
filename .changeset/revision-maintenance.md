---
default: patch
---

# Keep earlier exports tracked during maintenance

Earlier versions remain recorded so Clean preserves them and Consolidate moves
them with their asset. Exports also recreate deleted destination folders and
handle duplicate detection safely when revisions export concurrently.
Consolidate handles overlapping revision filenames, and Dates and Consolidate
can use metadata from earlier versions when the latest export is missing.
Failed destination creation no longer reserves an unusable export location.
