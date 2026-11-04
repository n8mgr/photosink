---
default: patch
---

# Applying dates no longer stops at the first failing asset

When one chosen asset had no metadata or an unreadable date, Apply stopped there and the assets already redated kept their new dates but were never moved to their new folders. Each failure is now listed on its row and the rest are redated and moved.
