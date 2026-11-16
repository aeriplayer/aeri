//! A test-only allocator that fails a test when guarded code touches the heap (ADR 0018).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static GUARDED: Cell<bool> = const { Cell::new(false) };
    static VIOLATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

fn note_heap_use() {
    if GUARDED.with(Cell::get) {
        VIOLATIONS.with(|count| count.set(count.get() + 1));
    }
}

#[expect(unsafe_code, reason = "a global allocator is an unsafe trait impl")]
// SAFETY: every method forwards to `System` with the caller's own arguments, so `System`
// upholds `GlobalAlloc`'s contract; the bookkeeping touches no heap memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_heap_use();
        // SAFETY: the caller upholds `alloc`'s contract for `layout`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_heap_use();
        // SAFETY: the caller upholds `alloc_zeroed`'s contract for `layout`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note_heap_use();
        // SAFETY: `ptr` came from this allocator, which got it from `System`, with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_heap_use();
        // SAFETY: `ptr` came from `System` with `layout`; the caller upholds `new_size`'s rules.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Runs `f`, and panics if it allocated, reallocated, or freed on this thread.
pub(crate) fn assert_no_alloc<T>(f: impl FnOnce() -> T) -> T {
    VIOLATIONS.with(|count| count.set(0));
    GUARDED.with(|guarded| guarded.set(true));
    let result = f();
    GUARDED.with(|guarded| guarded.set(false));
    let violations = VIOLATIONS.with(Cell::get);
    assert_eq!(
        violations, 0,
        "guarded code touched the heap {violations} times"
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_allocation_inside_the_guard_fails_it() {
        let caught = std::panic::catch_unwind(|| assert_no_alloc(|| Vec::<u8>::with_capacity(8)));
        assert!(caught.is_err());
    }

    #[test]
    fn heap_free_work_passes_the_guard() {
        assert_eq!(assert_no_alloc(|| [1_u8, 2, 3].iter().sum::<u8>()), 6);
    }
}
