# patches-core

Foundation crate for the Patches modular-audio framework: the
`Module` trait, descriptors, port and cable types, parameter
plumbing, and the runtime module registry.

Most authors should depend on [`patches-sdk`][sdk] rather than this
crate directly — `patches-sdk` is the supported public surface and
re-exports the items here. See
[ADR 0073](https://github.com/Vulpus-Labs/patches/blob/main/adr/0073-monorepo-split-into-successor-repos.md)
for the split rationale.

License: MIT.

[sdk]: https://crates.io/crates/patches-sdk
