# patches-ffi-common

ABI wire-format types, JSON descriptor schema, and the
`export_modules!` plugin macro shared between Patches hosts and
module bundles.

Module authors do not normally need to depend on this crate
directly — [`patches-sdk`][sdk] re-exports the macro and the
relevant items. Host implementors and authors of advanced single-
module plugins reach into the `abi`, `port_frame`, and `sdk`
submodules here.

License: MIT.

[sdk]: https://crates.io/crates/patches-sdk
