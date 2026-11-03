//! Tracks which window the UI is currently working on, so window-scoped state (such as a
//! per-window theme override) can be looked up without threading a `WindowId` through every
//! call site.
//!
//! A window is "current" while a [`ViewContext`](crate::ViewContext) for one of its views is
//! alive, while one of its views renders, and while its scene is laid out and painted. Scopes
//! nest; the innermost one wins.

use std::cell::RefCell;

use super::WindowId;

thread_local! {
    static STACK: RefCell<Vec<(u64, WindowId)>> = const { RefCell::new(Vec::new()) };
    static NEXT_TOKEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The window whose views are being updated, rendered or painted on this thread, if any.
pub fn current_window_id() -> Option<WindowId> {
    STACK.with(|stack| stack.borrow().last().map(|(_, window_id)| *window_id))
}

/// Runs `f` with `window_id` as the current window.
pub fn with_window_scope<R>(window_id: WindowId, f: impl FnOnce() -> R) -> R {
    let _scope = WindowScope::enter(window_id);
    f()
}

/// Marks a window as current until dropped.
///
/// Each scope removes exactly its own entry when dropped, so scopes that are dropped out of
/// order can't leave a stale window behind.
#[derive(Debug)]
pub struct WindowScope {
    token: u64,
}

impl WindowScope {
    pub fn enter(window_id: WindowId) -> Self {
        let token = NEXT_TOKEN.with(|next| {
            let token = next.get();
            next.set(token.wrapping_add(1));
            token
        });
        STACK.with(|stack| stack.borrow_mut().push((token, window_id)));
        Self { token }
    }
}

impl Drop for WindowScope {
    fn drop(&mut self) {
        // `try_with` because a scope may be dropped during thread-local teardown.
        let _ = STACK.try_with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(index) = stack.iter().rposition(|(token, _)| *token == self.token) {
                stack.remove(index);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_nest_and_unwind_out_of_order() {
        let a = WindowId::from_usize(1);
        let b = WindowId::from_usize(2);
        assert_eq!(current_window_id(), None);

        let outer = WindowScope::enter(a);
        assert_eq!(current_window_id(), Some(a));
        let inner = WindowScope::enter(b);
        assert_eq!(current_window_id(), Some(b));

        // Dropping the outer scope first must not disturb the inner one.
        drop(outer);
        assert_eq!(current_window_id(), Some(b));
        drop(inner);
        assert_eq!(current_window_id(), None);

        assert_eq!(with_window_scope(a, current_window_id), Some(a));
        assert_eq!(current_window_id(), None);
    }
}
