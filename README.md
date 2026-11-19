# win-uia2-nofocus

Minimal, audited safe Rust wrapper for creating a Windows `CUIAutomation8` COM client configured with `IUIAutomation2::SetAutoSetFocus(false)` and verified via `IUIAutomation2::AutoSetFocus()` readback before returning the client.

## Scope & Safety Boundary

This crate encapsulates **only** the narrow COM initialization and readback boundary required to prevent Windows UI Automation from implicitly stealing keyboard/window focus during semantic pattern actions (`ValuePattern.SetValue`, `InvokePattern.Invoke`, `SelectionItemPattern.Select`, etc.):

1. Initializes a thread-local COM multi-threaded apartment (`CoInitializeEx(None, COINIT_MULTITHREADED)`) balanced by a `!Send + !Sync` RAII `ComMtaGuard` (`CoUninitialize`).
2. Instantiates `CUIAutomation8` directly as `IUIAutomation2` via `CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)`. Never falls back to legacy `CUIAutomation`.
3. Disables automatic focus via `IUIAutomation2::SetAutoSetFocus(false)`.
4. Verifies via `IUIAutomation2::AutoSetFocus()` readback that automatic focus is `false`, failing closed otherwise.
5. Upcasts the exact same COM instance to `IUIAutomation` for seamless integration with crates such as `uiautomation` (`UIAutomation::from(raw)`).

This crate performs **no** physical input, **no** clipboard access, and **no** foreground focus manipulation or restoration.

## License

Dual-licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.
