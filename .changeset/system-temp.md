---
default: patch
---

# Stage exports in the system temporary directory

Exports now use the system temporary directory instead of creating `.tmp` in the
destination. Files can be placed on another drive, and temporary files are cleaned
up when the export finishes.
