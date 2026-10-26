//! The iOS main thread, reached from any thread through GCD.
//!
//! UIKit, AVFoundation, AVAudioSession and CoreAudio want to be called on the
//! main thread. Compiled on iOS whatever the video features, because the
//! Sentinel display probe needs it too.

unsafe extern "C" {
    /// The real symbol behind `dispatch_get_main_queue()` macro.
    /// `dispatch_get_main_queue()` is `#define dispatch_get_main_queue() (&_dispatch_main_q)`
    static _dispatch_main_q: std::ffi::c_void;

    fn dispatch_sync_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

unsafe extern "C" {
    /// Returns 1 if the calling thread is the main thread, 0 otherwise.
    fn pthread_main_np() -> std::ffi::c_int;
}

/// Execute a closure synchronously on the main thread via GCD.
///
/// This blocks the calling thread until `f` completes on the main queue.
/// Required for AVFoundation, AVAudioSession, and CoreAudio APIs on iOS.
///
pub(crate) fn run_on_main_thread<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    // If we're already on the main thread, just run directly.
    // (Calling dispatch_sync on the main queue from the main thread deadlocks.)
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }

    // Pack the closure and result slot into a context struct on the stack.
    type PanicPayload = Box<dyn std::any::Any + Send + 'static>;

    struct Context<F, R> {
        f: Option<F>,
        result: Option<std::result::Result<R, PanicPayload>>,
    }

    let mut ctx = Context {
        f: Some(f),
        result: None,
    };

    unsafe extern "C" fn trampoline<F, R>(raw: *mut std::ffi::c_void)
    where
        F: FnOnce() -> R,
    {
        // SAFETY: `run_on_main_thread` passes a live, uniquely borrowed stack
        // `Context<F, R>` to `dispatch_sync_f`, which invokes this callback
        // synchronously before returning. The callback neither retains nor
        // aliases the pointer beyond this call.
        let ctx = &mut *(raw as *mut Context<F, R>);
        if let Some(f) = ctx.f.take() {
            // A Rust panic must never unwind through libdispatch's C frames.
            // Capture it here, then resume it on the original Rust caller
            // after `dispatch_sync_f` has returned.
            ctx.result = Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)));
        }
    }

    unsafe {
        // SAFETY: `_dispatch_main_q` is libdispatch's process-lifetime main
        // queue. `ctx` remains live and exclusively borrowed for the complete
        // synchronous call, and `trampoline` catches all Rust unwinding before
        // returning across the C ABI.
        let main_queue = &_dispatch_main_q as *const std::ffi::c_void;
        dispatch_sync_f(
            main_queue,
            &mut ctx as *mut Context<F, R> as *mut std::ffi::c_void,
            trampoline::<F, R>,
        );
    }

    match ctx
        .result
        .expect("dispatch_sync_f returned without invoking its callback")
    {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
