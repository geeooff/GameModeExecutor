# Lot 10 — Configuration window

**Status: proposed; set aside on 2026-09-25** until a UX/UI analysis
refines it — nothing here is mature enough to start from. The first lot with a real window. It edits the
configuration through a UI, so hand-editing mistakes — doubled backslashes
above all — stop being possible.

Staying on TOML reopens something that was closed when JSON was considered:
`toml_edit` round-trips a file while preserving comments and layout, so a
configuration window could save without destroying what the user wrote.
Losing comments would still be acceptable; it may no longer be necessary.

To settle when it is taken: it supersedes the *Edit configuration* menu entry
from [Lot 6](06-notification-icon.md), which should then open the window
rather than the shell — and it should write through the same stage-validate-
promote path as [Lot 12](12-editing-on-a-copy.md) rather than become a third
way of writing the file.
