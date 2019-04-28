#![no_std]
#![feature(const_fn)]
#![feature(test)]

//! Call closures from interrupt handlers.
//!
//! # Motivation
//!
//! Existing solutions for interrupt handlers typically revolve around
//! wrapping resources needed by the handler in an `Option`, wrapped
//! in a `RefCell` wrapped in an `Mutex`, incurring some run-time
//! overhead every time the resource is required in the interrupt
//! handler, in addition to a fair amount of boilerplate. This module
//! attempts to leverage Rust's borrow checker and move semantics to
//! allow interrupt handlers to directly use their resources with a
//! minimum of overhead.
//!
//! To accomplish this, we use a closure which is called by an
//! interrupt handler. Because the closure has access to its
//! environment, we can use `move`, references, and mutable references
//! to ensure that variables are available as necessary to the
//! interrupt handler, while leveraging the borrow checker to ensure
//! safety at compile time. The only overhead is what it takes to call
//! the closure itself.
//!
//! # Safety
//!
//! While this module endeavors to use Rust's safety guarantees to
//! allow for use of resources inside interrupt handlers, due to
//! expected use-cases, closure semantics, and how interrupt handlers
//! are invoked from hardware, certain operations cannot be done
//! safely at this level.
//!
//! Notably, for the handler to be useful when called from interrupt
//! context, it needs to be stored in a `static mut` variable. This
//! means that the closure you supply it must also be effectively
//! `static` or replaced with a longer-lived closure before it goes
//! out of scope. `Handler::default_handler()` is provided for this
//! purpose.
//!
//! Additionally, replacement of an interrupt handler's closure may
//! race with the calling of the interrupt handler's closure (i.e.,
//! `Handler.replace()` may happen concurrently with
//! `Handler.call()`). You need to avoid this situation however is
//! appropriate for your code. The expected usage would be replacing
//! the handler`s closure once, while interrupts are disabled, thus
//! preventing the simultaneous replace/call problem. As this module
//! makes no assumptions about the environment in which it will be
//! used, this cannot be done for you.
//!
//! # Examples
//!
//! This example for an ARM Cortex-M system demonstrates safe usage by
//! only replacing the closure for `SYSTICK_HANDLER` inside a critical
//! section obtained by `cortex_m::interrupt::free()`, and shows how
//! it is called via the `SysTick()` function, which is called
//! directly from hardware.
//!
//! ``` no_run
//! use clint::Handler;
//! use cortex_m_rt::exception;
//!
//! static mut SYSTICK_HANDLER: Handler = Handler::new();
//!
//! fn main() {
//!     // NB: `closure` is in the lexical scope of `main`, and thus
//!     // cannot go out of scope.
//!     let closure = || {
//!         // Your interrupt handling code.
//!     };
//!     // Replace the handler for SysTick with closure while interrupts are
//!     // disabled.
//!     cortex_m::interrupt::free(|_| {
//!         unsafe { SYSTICK_HANDLER.replace(&closure) };
//!     });
//!
//!     loop {
//!         // Your main loop.
//!     }
//! }
//!
//! #[exception]
//! fn SysTick() {
//!     unsafe { SYSTICK_HANDLER.call() };
//! }
//! ```

pub struct Handler {
    // Handler that will be executed on `call`.
    h: *const dyn FnMut(),
}

impl Handler {
    /// Returns a new Handler that initially does nothing when
    /// called. Override it's behavior by using `replace`.
    pub const fn new() -> Self {
        Self {
            h: &Self::default_handler,
        }
    }

    /// Replace the behavior of this handler with `f`.
    ///
    /// # Safety
    ///
    /// There is no exclusion on replacing the handler's behavior
    /// while it is being executed. It is your responsibility to make
    /// sure that it's not being executed when you call `replace`.
    pub unsafe fn replace<F>(&mut self, f: &F)
    where
        F: FnMut() + 'static,
    {
        self.h = f;
    }

    /// Execute this handler.
    ///
    /// # Safety
    ///
    /// This function assumes that a replace is not occurring when the
    /// closure is being looked up. You need to ensure that `replace`
    /// and `call` can not occur at the same time.
    pub unsafe fn call(&self) {
        let f: &mut dyn FnMut() = &mut *(self.h as *mut dyn FnMut());
        f();
    }

    /// Do nothing handler. Needed by `call` until `replace` is used
    /// to set specific behavior. Can also be used to replace a
    /// closure that is about to go out of scope.
    pub fn default_handler() {}
}

impl core::fmt::Debug for Handler {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let (f0, f1) = unsafe { core::mem::transmute::<_, (usize, usize)>(self.h) };
        write!(f, "Handler{{ h: (0x{:x}, 0x{:x}) }}", f0, f1)
    }
}

#[cfg(test)]
mod test {
    extern crate test;
    use super::*;
    use test::Bencher;

    #[test]
    fn replace() {
        static mut X: usize = 0;

        let mut handler = Handler::new();
        unsafe {
            handler.replace(&|| X += 1);
            assert_eq!(X, 0);
            handler.call();
            handler.call();
            assert_eq!(X, 2);
        }
    }

    #[test]
    fn replace_static() {
        static mut HANDLER: Handler = Handler::new();
        static mut X: usize = 0;

        unsafe {
            HANDLER.replace(&|| X += 1);
            assert_eq!(X, 0);
            HANDLER.call();
            HANDLER.call();
            assert_eq!(X, 2);
        }
    }

    #[test]
    fn replace_with_default() {
        let mut handler = Handler::new();
        unsafe {
            handler.replace(&Handler::default_handler);
            handler.call()
        }
    }

    const ITER_COUNT: usize = 10_000;

    #[bench]
    fn bench_bare_fn(b: &mut Bencher) {
        static mut X: usize = 0;
        #[inline(never)]
        fn inc() {
            unsafe { X += 1 };
        }

        let n = test::black_box(ITER_COUNT);
        b.iter(|| (0..n).for_each(|_| inc()));
        assert!(unsafe { X } > 0);
    }

    #[bench]
    fn bench_handler(b: &mut Bencher) {
        static mut X: usize = 0;
        let mut handler = Handler::new();
        unsafe { handler.replace(&move || X += 1) };

        let n = test::black_box(ITER_COUNT);
        b.iter(|| (0..n).for_each(|_| unsafe { handler.call() }));
        assert!(unsafe { X } > 0);
    }
}