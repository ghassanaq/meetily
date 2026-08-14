//! Serialises Windows `cpal` device queries onto one process-lifetime thread.
//!
//! # Why this exists
//!
//! cpal caches one `IMMDeviceEnumerator` in a process-global `OnceLock`
//! (`cpal/src/host/wasapi/device.rs`), but it initialises COM *per thread*: a
//! thread-local guard calls `CoInitializeEx` on first use and `CoUninitialize`
//! when the thread exits. The cached enumerator therefore belongs to whichever
//! thread happened to make the first cpal call.
//!
//! When that thread exits, `CoUninitialize` tears down the last apartment in the
//! process and COM is free to unload the audio in-proc server. The cached
//! enumerator's vtable pointer is then dangling, and the next caller faults with
//! `STATUS_ACCESS_VIOLATION` (0xC0000005) while reading it. An access violation
//! is not a Rust panic and not an `Err` - it kills the process, so no amount of
//! `Result` handling at the call site can contain it.
//!
//! The window is easiest to hit when a thread builds a device iterator without
//! draining it (`host.output_devices().is_ok()`, as
//! [`check_system_audio_permissions`](crate::audio::check_system_audio_permissions)
//! does): nothing activates an `IAudioClient`, so nothing keeps the audio DLL
//! pinned across the teardown.
//!
//! Upstream bug: <https://github.com/RustAudio/cpal/issues/1302>. It is open
//! against cpal `main`, and the code is identical in the 0.15.3 we build
//! against, so upgrading cpal does not fix it.
//!
//! # The fix
//!
//! On Windows, own a dedicated, long-lived thread and make every cpal query from
//! it. This is the comprehensive option listed in the upstream issue. The
//! thread is spawned on first use, never exits, and survives panics in the
//! closures it runs, so the apartment that owns the cached enumerator outlives
//! every other thread in the process. On other platforms the wrapper executes
//! inline, preserving their existing audio-thread behavior.

#[cfg(target_os = "windows")]
use std::cell::Cell;
#[cfg(target_os = "windows")]
use std::sync::mpsc::{channel, Sender};
#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
type Job = Box<dyn FnOnce() + Send + 'static>;

/// Sender to the audio host thread. Held in a `static` forever, so the thread's
/// `recv()` never returns `Err` and the thread never exits.
#[cfg(target_os = "windows")]
static JOBS: OnceLock<Sender<Job>> = OnceLock::new();

#[cfg(target_os = "windows")]
thread_local! {
    /// Set only on the audio host thread, so nested calls run inline instead of
    /// deadlocking on a thread that is already busy running us.
    static IS_HOST_THREAD: Cell<bool> = const { Cell::new(false) };
}

#[cfg(target_os = "windows")]
fn jobs() -> &'static Sender<Job> {
    JOBS.get_or_init(|| {
        let (sender, receiver) = channel::<Job>();

        std::thread::Builder::new()
            .name("meetily-audio-host".to_string())
            .spawn(move || {
                IS_HOST_THREAD.with(|flag| flag.set(true));
                // Runs until the process exits; `JOBS` keeps the sender alive.
                while let Ok(job) = receiver.recv() {
                    job();
                }
            })
            .expect("failed to spawn audio host thread");

        sender
    })
}

/// Runs `f` on the audio host thread and blocks until it returns.
///
/// Every `cpal` host, device, and configuration query must go through here.
/// Calling cpal directly from an arbitrary thread risks orphaning cpal's cached
/// COM enumerator; see the module docs.
///
/// Panics inside `f` propagate to the caller and leave the host thread running.
#[cfg(target_os = "windows")]
pub fn on_audio_host_thread<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // Already on the host thread (a cpal call that re-entered our own helpers):
    // run inline, otherwise we would wait for a thread that is waiting for us.
    if IS_HOST_THREAD.with(|flag| flag.get()) {
        return f();
    }

    let (result_tx, result_rx) = channel();

    jobs()
        .send(Box::new(move || {
            // Keep the host thread alive across a panicking job: if it unwound,
            // the apartment owning cpal's enumerator would die with it and we
            // would be back to the access violation this module exists to fix.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            let _ = result_tx.send(result);
        }))
        .expect("audio host thread is gone");

    match result_rx.recv().expect("audio host thread is gone") {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Runs `f` inline on platforms unaffected by the Windows COM lifetime bug.
#[cfg(not(target_os = "windows"))]
#[inline]
pub fn on_audio_host_thread<T, F>(f: F) -> T
where
    F: FnOnce() -> T,
{
    f()
}

/// Debug-build guard for code that calls cpal directly.
///
/// Put this at the top of any helper that queries cpal but is not itself
/// wrapped in [`on_audio_host_thread`], so a call added on the wrong thread
/// fails loudly in development instead of becoming an access violation in a
/// user's release build.
#[inline]
#[cfg(target_os = "windows")]
pub fn debug_assert_on_audio_host_thread() {
    debug_assert!(
        IS_HOST_THREAD.with(|flag| flag.get()),
        "cpal was called off the audio host thread; wrap the call in \
         audio::host_thread::on_audio_host_thread"
    );
}

/// No-op guard on platforms unaffected by the Windows COM lifetime bug.
#[inline]
#[cfg(not(target_os = "windows"))]
pub fn debug_assert_on_audio_host_thread() {}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait};
    use std::thread::ThreadId;

    /// Regression test for the reported `cargo test --workspace` abort
    /// (0xC0000005) in system-audio device enumeration.
    ///
    /// Reproduces the original sequence directly: one short-lived thread builds
    /// a device iterator and drops it *undrained* (what
    /// `check_system_audio_permissions` does), exits so its thread-local COM
    /// guard runs `CoUninitialize`, and then a second thread enumerates. Before
    /// the fix this faulted while reading the cached `IMMDeviceEnumerator`
    /// vtable and took the whole test binary down with it.
    ///
    /// A regression here does not show up as a failed assertion - the process
    /// aborts with exit code 0xC0000005 and every other test in the binary is
    /// reported as not run.
    ///
    /// Runs on any machine; it asserts nothing about how many devices exist, so
    /// it is valid with zero audio endpoints (RDP without audio redirection, a
    /// VM, or a disabled output device).
    #[test]
    fn enumeration_survives_the_probing_thread_exiting() {
        std::thread::spawn(|| {
            let probed = on_audio_host_thread(|| cpal::default_host().output_devices().is_ok());
            println!("probe reported hosts available: {}", probed);
        })
        .join()
        .expect("probe thread panicked");

        let names = std::thread::spawn(|| {
            on_audio_host_thread(|| {
                let host = cpal::default_host();
                let Ok(devices) = host.output_devices() else {
                    return Vec::new();
                };
                devices
                    .filter_map(|device| device.name().ok())
                    .collect::<Vec<_>>()
            })
        })
        .join()
        .expect("enumeration thread panicked");

        println!("enumerated {} output devices", names.len());
    }

    /// Repeated thread churn against the same cached enumerator.
    ///
    /// Kept short on purpose: `output_devices()` activates an `IAudioClient` per
    /// device to filter the list, so each pass costs on the order of half a
    /// second on real hardware.
    #[test]
    fn enumeration_survives_repeated_thread_churn() {
        for _ in 0..10 {
            std::thread::spawn(|| {
                on_audio_host_thread(|| {
                    let host = cpal::default_host();
                    host.output_devices().map(|d| d.count()).unwrap_or(0)
                })
            })
            .join()
            .expect("churn thread panicked");
        }
    }

    #[test]
    fn all_work_lands_on_one_thread_that_is_not_the_caller() {
        let caller = std::thread::current().id();

        let first = on_audio_host_thread(|| std::thread::current().id());
        let second = std::thread::spawn(|| on_audio_host_thread(|| std::thread::current().id()))
            .join()
            .expect("thread panicked");

        assert_ne!(first, caller, "work must not run on the calling thread");
        assert_eq!(first, second, "all work must share one host thread");
    }

    #[test]
    fn nested_calls_run_inline_instead_of_deadlocking() {
        let (outer, inner): (ThreadId, ThreadId) = on_audio_host_thread(|| {
            (
                std::thread::current().id(),
                on_audio_host_thread(|| std::thread::current().id()),
            )
        });

        assert_eq!(
            outer, inner,
            "a nested call must run inline on the host thread"
        );
    }

    #[test]
    fn a_panicking_job_does_not_kill_the_host_thread() {
        let before = on_audio_host_thread(|| std::thread::current().id());

        let panicked = std::panic::catch_unwind(|| on_audio_host_thread(|| panic!("job blew up")));
        assert!(panicked.is_err(), "the panic must reach the caller");

        let after = on_audio_host_thread(|| std::thread::current().id());
        assert_eq!(
            before, after,
            "the host thread must survive a panicking job"
        );
    }
}
