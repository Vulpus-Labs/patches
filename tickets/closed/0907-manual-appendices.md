---
id: "0907"
title: Manual — appendices (glossary, changelog)
priority: low
created: 2026-05-17
epic: E149
---

## Summary

Write the two appendices.

## Acceptance criteria

- [ ] `docs/src/glossary.md` — terms used across the manual: patch,
      module, port, cable, signal kinds (audio, CV, gate, trigger,
      stereo), mono / poly, V/oct, voice allocation, hot-reload,
      fusion, SCC, plan, plugin (CLAP vs FFI native plugin), DSL,
      template, sidecar, global config. One or two sentences per
      term.
- [ ] `docs/src/changelog.md` — decide on approach. Options:
      1. duplicate content from repo-root `RELEASE_NOTES.md`
         (risks drift);
      2. drop the stub, link to `RELEASE_NOTES.md` from SUMMARY
         appendix instead;
      3. generate changelog.md from RELEASE_NOTES.md at build time.
      Recommend option 2 unless mdbook can include external files
      cleanly. If dropped, remove from `SUMMARY.md` to match.

## Notes

- If changelog approach changes, `SUMMARY.md` needs a coordinated
  edit — Part I (introduction, mental-model) and SUMMARY are
  already in good shape; touch SUMMARY only to remove the
  changelog entry if dropping the stub.
