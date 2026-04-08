# E046 — patches-dsl test coverage gaps

## Goal

Fill the coverage gaps identified in the patches-dsl test report. The crate has
84 passing tests but several important categories are untested or only
superficially tested: fixture content verification, literal edge cases, error
location accuracy, shape arguments, structural edge cases (duplicates, diamonds,
empty bodies, zero arity), enum declarations, poly connections, warning
generation, and scale composition across both boundaries simultaneously.

## Background

A systematic review of the test report (see `patches-dsl/docs/test-report.md`)
identified 17 gap areas, grouped into 7 tickets:

1. **Parser fixture content & literal edge cases** — fixtures are parsed but
   their AST output is never inspected; dB and note literals lack edge-case
   coverage.
2. **Parse error location accuracy** — errors are tested for existence but
   line/column is never verified.
3. **Shape argument inspection** — `FlatModule::shape` is never asserted on
   despite appearing in many fixtures.
4. **Structural edge cases** — unused templates, empty bodies, duplicate module
   IDs, diamond wiring, zero-arity groups.
5. **Enum declarations & poly connections** — `EnumDecl` is exported but
   untested; poly field on connections is untested.
6. **Warning generation** — only tested for absence, never for presence.
7. **Scale composition completeness** — no test combines non-trivial in-boundary
   and out-boundary scales on the same template simultaneously.

## Tickets

| # | Title | Priority |
|---|-------|----------|
| 0247 | Parser fixture content and literal edge cases | medium |
| 0248 | Parse error location accuracy | low |
| 0249 | Shape argument verification in expander tests | medium |
| 0250 | Structural edge cases (duplicates, diamonds, empty, zero arity) | medium |
| 0251 | Enum declarations and poly connection coverage | medium |
| 0252 | Warning generation tests | low |
| 0253 | Bidirectional scale composition and depth stress | low |

## Non-goals

- Property-based / fuzz testing. Valuable but a separate initiative requiring
  a proptest dependency and a generator for valid patch sources.
- Template depth stress testing beyond ~10 levels. The expander uses recursion
  with a cycle-detection set; pathological depth is a correctness concern only
  if the stack overflows, which is unlikely at realistic depths.
