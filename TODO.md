# TODO

## Settings integration cleanup

The current Settings → theme integration works but isn't the cleanest:

- Base theme + workspace-colour drafts, live preview, persistence enablement,
  and curated-profile selection are spread across `settings.rs`,
  `settings_overlay.rs`, `state.rs`, `config.rs`, and `colours.rs`.
- Preview/cancel/apply has to juggle draft theme names, draft colour flags,
  palette notices, per-theme curated worlds, render keys, and fallback to
  Original palettes.

## Themed fonts/colours only

Ideally we don't expose "all" font/colour variations in settings.

Instead:

- Keep only themed fonts/colours.
- Choose a small set of foreground/background combinations that are known to
  work with each background theme.
- Remove or hide the full catalogue / "Original palettes" path from normal
  settings once the curated per-theme sets are good enough.
- Keep any full-catalogue escape hatch (if needed) out of the main flow, e.g.
  config-only or advanced-only.
