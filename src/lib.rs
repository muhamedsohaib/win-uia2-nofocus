//! Minimal, audited safe Rust wrapper for creating a Windows `CUIAutomation8`
//! COM client configured with `IUIAutomation2::SetAutoSetFocus(false)` and
//! verified via readback before exposing the client.
//!
//! # Safety Boundary
//!
//! This crate intentionally encapsulates only the narrow COM lifecycle needed
//! to create and verify a focus-safe Windows UI Automation client:
//!
//! 1. `CoInitializeEx(None, COINIT_MULTITHREADED)` paired with a `!Send + !Sync`
//!    RAII [`ComMtaGuard`] that balances initialization with `CoUninitialize`.
//! 2. `CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)` directly
//!    as `IUIAutomation2` (never falling back to legacy `CUIAutomation`).
//! 3. `IUIAutomation2::SetAutoSetFocus(false)`.
//! 4. `IUIAutomation2::AutoSetFocus()` readback verification (`false` required).
//! 5. Safe upcast from `IUIAutomation2` to `IUIAutomation` so downstream crates
//!    such as `uiautomation` can operate on the exact same COM instance via
//!    `UIAutomation::from(raw)`.
//!
//! This crate never sends physical input, never touches the clipboard, and
//! never alters or restores foreground window focus.

use std::fmt;

/// `E_FAIL` (`0x8000_4005`), observed during transient Windows UI Automation
/// lazy initialization races.
const HRESULT_E_FAIL: i32 = i32::from_ne_bytes(0x8000_4005_u32.to_ne_bytes());

/// `RPC_E_CALL_REJECTED` (`0x8001_0001`).
const HRESULT_RPC_E_CALL_REJECTED: i32 = i32::from_ne_bytes(0x8001_0001_u32.to_ne_bytes());

/// `RPC_E_SERVERCALL_RETRYLATER` (`0x8001_010A`).
const HRESULT_RPC_E_SERVERCALL_RETRYLATER: i32 = i32::from_ne_bytes(0x8001_010A_u32.to_ne_bytes());

/// Typed failure modes for fail-closed `CUIAutomation8` / `IUIAutomation2`
/// focus-safe client initialization and readback verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Uia2Error {
    /// `CoInitializeEx(None, COINIT_MULTITHREADED)` returned a failing `HRESULT`.
    ComInitFailed {
        /// Raw Win32 `HRESULT` value.
        hresult: i32,
        /// OS error description.
        message: String,
    },
    /// `CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)` failed.
    CreateClientFailed {
        /// Raw Win32 `HRESULT` value.
        hresult: i32,
        /// OS error description.
        message: String,
    },
    /// Querying `IUIAutomation2` on an existing `IUIAutomation` instance failed.
    InterfaceQueryFailed {
        /// Raw Win32 `HRESULT` value.
        hresult: i32,
        /// OS error description.
        message: String,
    },
    /// `IUIAutomation2::SetAutoSetFocus(false)` returned an error.
    DisableAutoSetFocusFailed {
        /// Raw Win32 `HRESULT` value.
        hresult: i32,
        /// OS error description.
        message: String,
    },
    /// `IUIAutomation2::AutoSetFocus()` readback failed.
    VerifyAutoSetFocusFailed {
        /// Raw Win32 `HRESULT` value.
        hresult: i32,
        /// OS error description.
        message: String,
    },
    /// `IUIAutomation2::AutoSetFocus()` read back `true` after calling
    /// `SetAutoSetFocus(false)`.
    AutoSetFocusStillEnabled,
}

impl Uia2Error {
    /// Returns the raw Win32 `HRESULT` associated with this error, if any.
    #[must_use]
    pub const fn hresult(&self) -> Option<i32> {
        match self {
            Self::ComInitFailed { hresult, .. }
            | Self::CreateClientFailed { hresult, .. }
            | Self::InterfaceQueryFailed { hresult, .. }
            | Self::DisableAutoSetFocusFailed { hresult, .. }
            | Self::VerifyAutoSetFocusFailed { hresult, .. } => Some(*hresult),
            Self::AutoSetFocusStillEnabled => None,
        }
    }

    /// Returns `true` only for known transient Windows COM/UIA initialization
    /// failures (`E_FAIL`, `RPC_E_CALL_REJECTED`, `RPC_E_SERVERCALL_RETRYLATER`)
    /// during client creation.
    ///
    /// Permanent incompatibilities (such as `REGDB_E_CLASSNOTREG`,
    /// `E_NOINTERFACE`, `RPC_E_CHANGED_MODE`, or `AutoSetFocusStillEnabled`)
    /// always return `false`.
    #[must_use]
    pub const fn is_transient_init_error(&self) -> bool {
        match self {
            Self::CreateClientFailed { hresult, .. } => matches!(
                *hresult,
                HRESULT_E_FAIL | HRESULT_RPC_E_CALL_REJECTED | HRESULT_RPC_E_SERVERCALL_RETRYLATER
            ),
            Self::ComInitFailed { .. }
            | Self::InterfaceQueryFailed { .. }
            | Self::DisableAutoSetFocusFailed { .. }
            | Self::VerifyAutoSetFocusFailed { .. }
            | Self::AutoSetFocusStillEnabled => false,
        }
    }
}

impl fmt::Display for Uia2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComInitFailed { hresult, message } => write!(
                f,
                "COM MTA initialization failed ({}): {message}",
                format_hresult(*hresult)
            ),
            Self::CreateClientFailed { hresult, message } => write!(
                f,
                "CUIAutomation8 (IUIAutomation2) creation failed ({}): {message}",
                format_hresult(*hresult)
            ),
            Self::InterfaceQueryFailed { hresult, message } => write!(
                f,
                "IUIAutomation2 interface query failed ({}): {message}",
                format_hresult(*hresult)
            ),
            Self::DisableAutoSetFocusFailed { hresult, message } => write!(
                f,
                "IUIAutomation2::SetAutoSetFocus(false) failed ({}): {message}",
                format_hresult(*hresult)
            ),
            Self::VerifyAutoSetFocusFailed { hresult, message } => write!(
                f,
                "IUIAutomation2::AutoSetFocus() readback failed ({}): {message}",
                format_hresult(*hresult)
            ),
            Self::AutoSetFocusStillEnabled => write!(
                f,
                "IUIAutomation2::AutoSetFocus() remained true after SetAutoSetFocus(false)"
            ),
        }
    }
}

impl std::error::Error for Uia2Error {}

/// Validates a readback value from `IUIAutomation2::AutoSetFocus()`.
///
/// # Errors
///
/// Returns [`Uia2Error::AutoSetFocusStillEnabled`] if `observed_auto_set_focus`
/// is `true`.
pub const fn validate_auto_set_focus_readback(
    observed_auto_set_focus: bool,
) -> Result<(), Uia2Error> {
    if observed_auto_set_focus {
        Err(Uia2Error::AutoSetFocusStillEnabled)
    } else {
        Ok(())
    }
}

fn format_hresult(hresult: i32) -> String {
    format!("0x{:08X}", u32::from_ne_bytes(hresult.to_ne_bytes()))
}

#[cfg(windows)]
mod imp {
    use std::marker::PhantomData;

    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation8, IUIAutomation, IUIAutomation2};
    use windows::core::Interface;

    use crate::{Uia2Error, validate_auto_set_focus_readback};

    /// RAII guard for a thread's COM multi-threaded apartment (`COINIT_MULTITHREADED`).
    ///
    /// This type is deliberately `!Send` and `!Sync` so that `CoUninitialize`
    /// always executes on the exact thread that called `CoInitializeEx`.
    ///
    /// Callers unpacking [`NoFocusUiaClient::into_parts`] MUST ensure all COM
    /// objects created on this thread (including `IUIAutomation`, tree walkers,
    /// and elements) are dropped before this guard is dropped.
    #[derive(Debug)]
    pub struct ComMtaGuard {
        _not_send_or_sync: PhantomData<*const ()>,
    }

    impl Drop for ComMtaGuard {
        fn drop(&mut self) {
            // SAFETY: `ComMtaGuard` is constructed exclusively after a successful
            // `CoInitializeEx(None, COINIT_MULTITHREADED)` (`S_OK` or `S_FALSE`),
            // and is `!Send + !Sync`, ensuring `CoUninitialize` balances that
            // initialization count exactly once on the same thread.
            unsafe {
                CoUninitialize();
            }
        }
    }

    /// A Windows `CUIAutomation8` COM client configured with
    /// `IUIAutomation2::SetAutoSetFocus(false)` and verified via readback.
    ///
    /// Field declaration order is load-bearing: `automation` is dropped before
    /// `apartment`, releasing the `IUIAutomation` COM pointer before
    /// `CoUninitialize` executes.
    #[derive(Debug)]
    pub struct NoFocusUiaClient {
        automation: IUIAutomation,
        apartment: ComMtaGuard,
    }

    impl NoFocusUiaClient {
        /// Initializes a COM multi-threaded apartment on the current thread,
        /// instantiates `CUIAutomation8` directly as `IUIAutomation2`, disables
        /// `AutoSetFocus`, and verifies `AutoSetFocus() == false` via readback.
        ///
        /// Fails closed on any error; never falls back to legacy `CUIAutomation`
        /// or an unverified client.
        ///
        /// # Errors
        ///
        /// Returns [`Uia2Error`] if COM MTA initialization fails,
        /// `CUIAutomation8` cannot be instantiated as `IUIAutomation2`,
        /// `SetAutoSetFocus(false)` fails, readback fails, or readback still
        /// reports `AutoSetFocus == true`.
        pub fn new_mta() -> Result<Self, Uia2Error> {
            // SAFETY: `CoInitializeEx(None, COINIT_MULTITHREADED)` initializes
            // COM for the calling thread without dereferencing raw pointers.
            // On `S_OK` or `S_FALSE`, `.ok()` succeeds and `apartment` balances
            // the initialization count via `CoUninitialize` on drop. On failure
            // (including `RPC_E_CHANGED_MODE`), COM does not increment the init
            // count and we return immediately without constructing `ComMtaGuard`.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .map_err(|err| Uia2Error::ComInitFailed {
                    hresult: err.code().0,
                    message: err.message(),
                })?;
            let apartment = ComMtaGuard {
                _not_send_or_sync: PhantomData,
            };

            // SAFETY: COM is initialized on the calling thread via `apartment`.
            // `CUIAutomation8` is a standard Windows 8+ in-process COM class
            // implementing `IUIAutomation2`. `CLSCTX_INPROC_SERVER` restricts
            // activation to in-process `UIAutomationCore.dll`.
            let automation2: IUIAutomation2 =
                unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }.map_err(
                    |err| Uia2Error::CreateClientFailed {
                        hresult: err.code().0,
                        message: err.message(),
                    },
                )?;

            // SAFETY: `automation2` is a valid, live `IUIAutomation2` COM
            // interface pointer on this thread's initialized MTA apartment.
            unsafe { automation2.SetAutoSetFocus(false) }.map_err(|err| {
                Uia2Error::DisableAutoSetFocusFailed {
                    hresult: err.code().0,
                    message: err.message(),
                }
            })?;

            // SAFETY: `automation2` is a valid, live `IUIAutomation2` COM
            // interface pointer on this thread's initialized MTA apartment.
            let observed = unsafe { automation2.AutoSetFocus() }
                .map_err(|err| Uia2Error::VerifyAutoSetFocusFailed {
                    hresult: err.code().0,
                    message: err.message(),
                })?
                .as_bool();
            validate_auto_set_focus_readback(observed)?;

            let automation = IUIAutomation::from(automation2);
            Ok(Self {
                automation,
                apartment,
            })
        }

        /// Borrows the underlying verified `IUIAutomation` COM client.
        #[must_use]
        pub const fn as_automation(&self) -> &IUIAutomation {
            &self.automation
        }

        /// Queries `IUIAutomation2::AutoSetFocus()` directly on this client's
        /// underlying COM instance.
        ///
        /// # Errors
        ///
        /// Returns [`Uia2Error`] if querying `IUIAutomation2` or reading
        /// `AutoSetFocus` fails.
        pub fn auto_set_focus(&self) -> Result<bool, Uia2Error> {
            query_auto_set_focus(&self.automation)
        }

        /// Splits this verified client into its raw `IUIAutomation` interface
        /// pointer (for adaptation via `uiautomation::UIAutomation::from`) and
        /// its [`ComMtaGuard`].
        ///
        /// The caller MUST keep the returned [`ComMtaGuard`] alive until after
        /// the `IUIAutomation` pointer and all derived UIA objects on this
        /// thread have been dropped.
        #[must_use]
        pub fn into_parts(self) -> (IUIAutomation, ComMtaGuard) {
            (self.automation, self.apartment)
        }
    }

    /// Reads `IUIAutomation2::AutoSetFocus()` on an existing `IUIAutomation`
    /// COM interface pointer.
    ///
    /// # Errors
    ///
    /// Returns [`Uia2Error::InterfaceQueryFailed`] if `automation` does not
    /// implement `IUIAutomation2` (for example, a legacy `CUIAutomation`
    /// instance), or [`Uia2Error::VerifyAutoSetFocusFailed`] if the COM call
    /// fails.
    pub fn query_auto_set_focus(automation: &IUIAutomation) -> Result<bool, Uia2Error> {
        let automation2: IUIAutomation2 =
            automation
                .cast()
                .map_err(|err| Uia2Error::InterfaceQueryFailed {
                    hresult: err.code().0,
                    message: err.message(),
                })?;

        // SAFETY: `automation2` is a valid, live `IUIAutomation2` COM
        // interface pointer obtained via `Interface::cast`.
        let enabled = unsafe { automation2.AutoSetFocus() }
            .map_err(|err| Uia2Error::VerifyAutoSetFocusFailed {
                hresult: err.code().0,
                message: err.message(),
            })?
            .as_bool();
        Ok(enabled)
    }
}

#[cfg(windows)]
pub use imp::{ComMtaGuard, NoFocusUiaClient, query_auto_set_focus};

#[cfg(test)]
mod tests {
    use super::{
        HRESULT_E_FAIL, HRESULT_RPC_E_CALL_REJECTED, HRESULT_RPC_E_SERVERCALL_RETRYLATER,
        Uia2Error, validate_auto_set_focus_readback,
    };

    #[test]
    fn readback_validation_accepts_false_and_rejects_true() {
        assert_eq!(validate_auto_set_focus_readback(false), Ok(()));
        assert_eq!(
            validate_auto_set_focus_readback(true),
            Err(Uia2Error::AutoSetFocusStillEnabled)
        );
    }

    #[test]
    fn transient_error_classification_is_strictly_bounded() {
        for hresult in [
            HRESULT_E_FAIL,
            HRESULT_RPC_E_CALL_REJECTED,
            HRESULT_RPC_E_SERVERCALL_RETRYLATER,
        ] {
            let err = Uia2Error::CreateClientFailed {
                hresult,
                message: "transient".into(),
            };
            assert!(err.is_transient_init_error());
            assert_eq!(err.hresult(), Some(hresult));
        }

        let class_not_reg = i32::from_ne_bytes(0x8004_0154_u32.to_ne_bytes());
        assert!(
            !Uia2Error::CreateClientFailed {
                hresult: class_not_reg,
                message: "Class not registered".into(),
            }
            .is_transient_init_error()
        );
        assert!(
            !Uia2Error::DisableAutoSetFocusFailed {
                hresult: HRESULT_E_FAIL,
                message: "failed".into(),
            }
            .is_transient_init_error()
        );
        assert!(!Uia2Error::AutoSetFocusStillEnabled.is_transient_init_error());
        assert_eq!(Uia2Error::AutoSetFocusStillEnabled.hresult(), None);
    }

    #[test]
    fn display_formats_hresult_hex_without_losing_context() {
        let err = Uia2Error::CreateClientFailed {
            hresult: HRESULT_E_FAIL,
            message: "Unspecified error".into(),
        };
        assert_eq!(
            err.to_string(),
            "CUIAutomation8 (IUIAutomation2) creation failed (0x80004005): Unspecified error"
        );
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use std::time::Duration;

    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoCreateInstance,
        CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

    use super::{NoFocusUiaClient, Uia2Error, query_auto_set_focus};

    static WINDOWS_UIA_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn creates_cuiautomation8_client_with_auto_set_focus_disabled() {
        let handle = thread::spawn(|| {
            let _guard = WINDOWS_UIA_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let client = NoFocusUiaClient::new_mta()
                .expect("CUIAutomation8 client with AutoSetFocus=false should initialize");
            assert_eq!(client.auto_set_focus(), Ok(false));
            assert_eq!(query_auto_set_focus(client.as_automation()), Ok(false));

            let (raw, com_guard) = client.into_parts();
            assert_eq!(query_auto_set_focus(&raw), Ok(false));
            drop(raw);
            drop(com_guard);
        });
        handle.join().expect("Windows UIA test thread panicked");
    }

    #[test]
    fn legacy_cuiautomation_client_does_not_report_auto_set_focus_disabled() {
        let handle = thread::spawn(|| {
            let _guard = WINDOWS_UIA_TEST_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // SAFETY: Initializes MTA COM on this dedicated test thread, balanced below.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .expect("MTA init should succeed");
            // SAFETY: COM is initialized on this thread; CUIAutomation is a standard Windows CLSID.
            let legacy: IUIAutomation =
                unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                    .expect("legacy CUIAutomation should instantiate");
            let query_result = query_auto_set_focus(&legacy);
            assert_ne!(
                query_result,
                Ok(false),
                "legacy CUIAutomation must never appear as AutoSetFocus=false"
            );
            drop(legacy);
            // SAFETY: Balances the successful `CoInitializeEx` above after all COM pointers dropped.
            unsafe {
                CoUninitialize();
            }
        });
        handle
            .join()
            .expect("legacy CUIAutomation test thread panicked");
    }

    #[test]
    fn sta_thread_fails_closed_without_calling_co_uninitialize() {
        let rpc_e_changed_mode = i32::from_ne_bytes(0x8001_0106_u32.to_ne_bytes());
        let handle = thread::spawn(move || {
            // SAFETY: Initializes STA COM on this dedicated test thread, balanced below.
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
                .ok()
                .expect("STA init should succeed");
            let err = NoFocusUiaClient::new_mta()
                .expect_err("STA thread must fail closed when MTA is requested");
            assert!(matches!(
                err,
                Uia2Error::ComInitFailed { hresult, .. } if hresult == rpc_e_changed_mode
            ));
            assert!(!err.is_transient_init_error());
            // SAFETY: Balances the STA `CoInitializeEx` call above.
            unsafe {
                CoUninitialize();
            }
        });
        handle.join().expect("STA test thread panicked");
    }

    #[test]
    fn concurrent_mta_threads_create_and_drop_no_focus_clients_cleanly() {
        const THREADS: usize = 4;
        let barrier = Arc::new(Barrier::new(THREADS));
        let mut handles = Vec::with_capacity(THREADS);

        for _ in 0..THREADS {
            let gate = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                gate.wait();
                let client = {
                    let mut last_err = None;
                    let mut created = None;
                    for attempt in 0..3 {
                        let guard = WINDOWS_UIA_TEST_LOCK
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        match NoFocusUiaClient::new_mta() {
                            Ok(c) => {
                                created = Some(c);
                                break;
                            }
                            Err(err) if err.is_transient_init_error() && attempt < 2 => {
                                last_err = Some(err);
                                drop(guard);
                                thread::sleep(Duration::from_millis(25));
                            }
                            Err(err) => panic!("unexpected UIA2 init error: {err}"),
                        }
                    }
                    created.unwrap_or_else(|| {
                        panic!("client creation failed after transient retries: {last_err:?}")
                    })
                };
                assert_eq!(client.auto_set_focus(), Ok(false));
                let _guard = WINDOWS_UIA_TEST_LOCK
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                drop(client);
            }));
        }

        for handle in handles {
            handle.join().expect("concurrent worker thread panicked");
        }
    }
}
