# Unsigned binaries

The Patches release binaries are unsigned. There is currently no
Apple Developer ID and no Windows code-signing certificate behind the
project. macOS and Windows both treat downloaded unsigned binaries
with suspicion, so first-run requires an explicit override.

This is policy on the publisher's side, not a security flaw on
yours — but the override mechanisms are real protections, so it's
worth knowing what each one does.

## macOS — Gatekeeper

When macOS downloads a file from the internet it tags it with an
extended attribute, `com.apple.quarantine`. Gatekeeper checks this
attribute on first launch: if the file is unsigned (or signed by an
unrecognised developer), the OS refuses to run it with a "cannot be
opened because the developer cannot be verified" dialog.

The release binaries are **ad-hoc signed** — `codesign --sign -` —
which satisfies the dyld loader (the dylib will resolve and link) but
not Gatekeeper (no trusted certificate). The quarantine flag must be
removed for first launch.

The release archive ships `macos-unquarantine.sh` for this:

```bash
./macos-unquarantine.sh
```

It runs `xattr -dr com.apple.quarantine` over `patch_player`,
`Patches.clap`, any bundled native module dylibs, and the bundled
`.vsix`. It also installs `Patches.clap` into
`~/Library/Audio/Plug-Ins/CLAP/` and, if the `code` CLI is on PATH,
installs the bundled VS Code extension. Steps that have no matching
input are skipped silently; the script is idempotent. Manual
equivalent for clearing a single file:

```bash
xattr -dr com.apple.quarantine /path/to/Patches.clap
```

`xattr -l <file>` lists extended attributes if you want to check
whether the flag is still set.

Once cleared, the binary launches normally — Gatekeeper only checks
on first launch. Re-downloading or re-extracting from the original
archive re-applies the flag.

## Windows — SmartScreen and Mark of the Web

Windows downloads from the internet get tagged with a *Mark of the
Web* (an alternate data stream named `Zone.Identifier`). When you
launch a tagged executable, SmartScreen interposes a *"Windows
protected your PC"* dialog and refuses to run unrecognised
unsigned binaries by default.

This applies equally to the **MSI installer** and to individual
binaries in the **zip archive** — the MSI is not code-signed, so
SmartScreen warns on it just as it does on the loose `.exe`. From
the warning dialog, click *More info → Run anyway* to proceed.

For loose files in the zip archive, the override is per-file:

1. Right-click the file (`.exe`, `.dll`, `.clap`, `.vsix`).
2. *Properties* → *General* tab.
3. Tick *Unblock* at the bottom.
4. *OK*.

This clears the Mark of the Web for that file. SmartScreen won't
prompt on subsequent launches.

PowerShell equivalent for scripting:

```powershell
Unblock-File -Path .\patch_player.exe
Unblock-File -Path .\Patches.clap
```

`Get-Item .\patch_player.exe -Stream Zone.Identifier` lists the
zone-identifier stream if it is still present.

## Linux

Linux distributions have no equivalent gating — the binary runs as
soon as it has the executable bit set. If you downloaded a build
artefact without the executable bit, restore it:

```bash
chmod +x patch_player
```

That's the whole "override".

## Why no signing?

Both Apple and Microsoft signing programs require ongoing developer
identity verification and a non-trivial annual fee. Patches is a
small project and signing has not been prioritised. When that
changes, these workarounds become unnecessary.

For now: clear the quarantine flag, tick *Unblock*, run the binary.
