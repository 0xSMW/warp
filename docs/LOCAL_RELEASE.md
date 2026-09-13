# Warp Local macOS releases

This fork ships the `warp` executable (`app/src/bin/local.rs`), with local-only
configuration, as `Warp.app`. Keep `dev.warp.Warp-Local` and the `warplocal` URL
scheme stable so installed preferences and permissions retain their identity.
The upstream `stable` executable and release workflows target Warp's cloud
product and infrastructure and are not the release path for this fork.

## Account and tools

Use `Developer ID Application: Resonant Media, Inc. (95FP69XZCG)` from the local
Keychain. `script/Entitlements.plist` and the runtime Apple Team ID must match.
Notarization uses the existing `asc` Keychain profile; `WARP_ASC_PROFILE` can
select another profile. Never export the private key or commit credentials.

Use the Rust toolchain pinned in `rust-toolchain.toml`, cargo-bundle 0.11.0,
cargo-about 0.8.4, Xcode command-line tools, `jq`, and `asc`.
Ensure `cargo` and `rustc` resolve through rustup rather than Homebrew's standalone
Rust installation. The release script rejects a mismatched compiler.
The command-signature build also needs the Yarn version pinned in
`crates/command-signatures-v2/js/package.json`. Ensure Corepack's Yarn shim is
available to the build; a conflicting Volta shim can prevent this dependency from building.

## Build and verify

Run the formatting and Clippy commands from `script/presubmit`, verify the
changed behavior, and commit each changed file separately with a specific message. From a clean
checkout, run:

```sh
bash script/macos/release-local 0.1.0
```

This builds for the current Mac's architecture, packages the local executable
and resources, signs with Hardened Runtime and a secure timestamp, notarizes
and staples the app, creates a drag-to-Applications DMG, then signs, notarizes,
staples, and checks that DMG. Output lives in
`target/local-release/<version>/`; existing output is never overwritten.
The plist records the source commit and release version.

Before publishing, mount the DMG read-only and verify its contained app with
`codesign --verify --deep --strict`, `xcrun stapler validate`, and `spctl`.
Install that exact app in `/Applications/Warp.app`, retaining a recoverable
backup of the previous installation. Preserve existing SQLite state before
changing the App Group container. Check restored sessions, shell execution,
hotkey-window behavior, and repeated cold launches. Confirm no repeated
`SystemPolicyAppData` prompt and no signature damage after icon changes.
Local icon selection must only change the running Dock image; Finder custom-icon metadata
on the signed bundle fails strict code-signature verification.

## GitHub publication

Push the verified commits to `0xSMW/warp`. Tag the tested source commit, create
a draft GitHub release with the matching version, and upload only the verified
DMG and `SHA256SUMS`. Download the draft asset and compare its SHA-256 before
publishing. Name the architecture explicitly; an arm64 build does not establish
Intel support. GitHub hosts the same artifact installed locally; it does not
rebuild or re-sign it. This path requires no Apple credentials in GitHub.
