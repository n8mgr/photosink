---
default: patch
---

# Takeout sidecars without edits, and library warnings in the run

Google Takeout imports with edits turned off now write the sidecar for photos that also have an edited copy; before, those photos were exported without their metadata. Apple Photos runs now show a warning in the run header when the library has no keyword table, album table or search index, instead of dropping the warning silently.
