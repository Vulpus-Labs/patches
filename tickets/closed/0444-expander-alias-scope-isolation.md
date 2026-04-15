---
id: "0444"
title: Expander alias-map scope isolation
priority: high
created: 2026-04-15
---

## Summary

`Expander.alias_maps` (`patches-dsl/src/expand.rs:810`) is built as a
`HashMap` on the expander and never cleared. Within a single `expand()`
call, aliases declared inside template A remain visible while expanding
an unrelated template B. No cross-template isolation.

Today this is latent: most patches don't exercise it because aliases
are typically scoped to the template that introduces them, and name
collisions are rare. But it is incorrect by construction — two
sibling template instantiations should not share alias state.

Scope the alias map per template-instantiation frame (push on entry,
pop on exit) so aliases only live as long as the template body that
declared them.

## Acceptance criteria

- [ ] Alias-map lifetime is bounded by template instantiation, not by
      the whole `expand()` run.
- [ ] Regression test: two sibling template calls declaring the same
      alias name with different targets each resolve their own alias
      correctly; the second call does not see the first's binding.
- [ ] Regression test: alias declared in a nested template does not
      leak to the outer template after the inner call returns.
- [ ] No change to the outer public API.
- [ ] `cargo test -p patches-dsl`, `cargo clippy` clean.

## Notes

Part of E082. Likely to uncover at least one test fixture that was
accidentally relying on the leaked state — adjust those tests as part
of this ticket and note the rationale in the commit.
