// Copyright 2012-2013 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Shareable mutable containers.
//!
//! Values of the `Cell` and `RefCell` types may be mutated through
//! shared references (i.e. the common `&T` type), whereas most Rust
//! types can only be mutated through unique (`&mut T`) references. We
//! say that `Cell` and `RefCell` provide *interior mutability*, in
//! contrast with typical Rust types that exhibit *inherited
//! mutability*.
//!
//! Cell types come in two flavors: `Cell` and `RefCell`. `Cell`
//! provides `get` and `set` methods that change the
//! interior value with a single method call. `Cell` though is only
//! compatible with types that implement `Copy`. For other types,
//! one must use the `RefCell` type, acquiring a write lock before
//! mutating.
//!
//! `RefCell` uses Rust's lifetimes to implement *dynamic borrowing*,
//! a process whereby one can claim temporary, exclusive, mutable
//! access to the inner value. Borrows for `RefCell`s are tracked *at
//! runtime*, unlike Rust's native reference types which are entirely
//! tracked statically, at compile time. Because `RefCell` borrows are
//! dynamic it is possible to attempt to borrow a value that is
//! already mutably borrowed; when this happens it results in task
//! panic.
//!
//! # When to choose interior mutability
//!
//! The more common inherited mutability, where one must have unique
//! access to mutate a value, is one of the key language elements that
//! enables Rust to reason strongly about pointer aliasing, statically
//! preventing crash bugs. Because of that, inherited mutability is
//! preferred, and interior mutability is something of a last
//! resort. Since cell types enable mutation where it would otherwise
//! be disallowed though, there are occasions when interior
//! mutability might be appropriate, or even *must* be used, e.g.
//!
//! * Introducing inherited mutability roots to shared types.
//! * Implementation details of logically-immutable methods.
//! * Mutating implementations of `clone`.
//!
//! ## Introducing inherited mutability roots to shared types
//!
//! Shared smart pointer types, including `Rc` and `Arc`, provide
//! containers that can be cloned and shared between multiple parties.
//! Because the contained values may be multiply-aliased, they can
//! only be borrowed as shared references, not mutable references.
//! Without cells it would be impossible to mutate data inside of
//! shared boxes at all!
//!
//! It's very common then to put a `RefCell` inside shared pointer
//! types to reintroduce mutability:
//!
//! ```
//! use std::collections::HashMap;
//! use std::cell::RefCell;
//! use std::rc::Rc;
//!
//! fn main() {
//!     let shared_map: Rc<RefCell<_>> = Rc::new(RefCell::new(HashMap::new()));
//!     shared_map.borrow_mut().insert("africa", 92388i);
//!     shared_map.borrow_mut().insert("kyoto", 11837i);
//!     shared_map.borrow_mut().insert("piccadilly", 11826i);
//!     shared_map.borrow_mut().insert("marbles", 38i);
//! }
//! ```
//!
//! Note that this example uses `Rc<T>` and not `Arc<T>`. `RefCell<T>`s are for single-threaded
//! scenarios. Consider using `Mutex<T>` if you need shared mutability in a multi-threaded
//! situation.
//!
//! ## Implementation details of logically-immutable methods
//!
//! Occasionally it may be desirable not to expose in an API that
//! there is mutation happening "under the hood". This may be because
//! logically the operation is immutable, but e.g. caching forces the
//! implementation to perform mutation; or because you must employ
//! mutation to implement a trait method that was originally defined
//! to take `&self`.
//!
//! ```
//! use std::cell::RefCell;
//!
//! struct Graph {
//!     edges: Vec<(uint, uint)>,
//!     span_tree_cache: RefCell<Option<Vec<(uint, uint)>>>
//! }
//!
//! impl Graph {
//!     fn minimum_spanning_tree(&self) -> Vec<(uint, uint)> {
//!         // Create a new scope to contain the lifetime of the
//!         // dynamic borrow
//!         {
//!             // Take a reference to the inside of cache cell
//!             let mut cache = self.span_tree_cache.borrow_mut();
//!             if cache.is_some() {
//!                 return cache.as_ref().unwrap().clone();
//!             }
//!
//!             let span_tree = self.calc_span_tree();
//!             *cache = Some(span_tree);
//!         }
//!
//!         // Recursive call to return the just-cached value.
//!         // Note that if we had not let the previous borrow
//!         // of the cache fall out of scope then the subsequent
//!         // recursive borrow would cause a dynamic task panic.
//!         // This is the major hazard of using `RefCell`.
//!         self.minimum_spanning_tree()
//!     }
//! #   fn calc_span_tree(&self) -> Vec<(uint, uint)> { vec![] }
//! }
//! ```
//!
//! ## Mutating implementations of `clone`
//!
//! This is simply a special - but common - case of the previous:
//! hiding mutability for operations that appear to be immutable.
//! The `clone` method is expected to not change the source value, and
//! is declared to take `&self`, not `&mut self`. Therefore any
//! mutation that happens in the `clone` method must use cell
//! types. For example, `Rc` maintains its reference counts within a
//! `Cell`.
//!
//! ```
//! use std::cell::Cell;
//!
//! struct Rc<T> {
//!     ptr: *mut RcBox<T>
//! }
//!
//! struct RcBox<T> {
//!     value: T,
//!     refcount: Cell<uint>
//! }
//!
//! impl<T> Clone for Rc<T> {
//!     fn clone(&self) -> Rc<T> {
//!         unsafe {
//!             (*self.ptr).refcount.set((*self.ptr).refcount.get() + 1);
//!             Rc { ptr: self.ptr }
//!         }
//!     }
//! }
//! ```
//!
// FIXME: Explain difference between Cell and RefCell
// FIXME: Downsides to interior mutability
// FIXME: Can't be shared between threads. Dynamic borrows
// FIXME: Relationship to Atomic types and RWLock

#![stable]

use clone::Clone;
use cmp::PartialEq;
use default::Default;
use marker::{Copy, Send};
use ops::{Deref, DerefMut, Drop};
use option::Option;
use option::Option::{None, Some};

/// A mutable memory location that admits only `Copy` data.
#[stable]
pub struct Cell<T> {
    value: UnsafeCell<T>,
}

impl<T:Copy> Cell<T> {
    /// Creates a new `Cell` containing the given value.
    #[stable]
    pub fn new(value: T) -> Cell<T> {
        Cell {
            value: UnsafeCell::new(value),
        }
    }

    /// Returns a copy of the contained value.
    #[inline]
    #[stable]
    pub fn get(&self) -> T {
        unsafe{ *self.value.get() }
    }

    /// Sets the contained value.
    #[inline]
    #[stable]
    pub fn set(&self, value: T) {
        unsafe {
            *self.value.get() = value;
        }
    }

    /// Get a reference to the underlying `UnsafeCell`.
    ///
    /// This can be used to circumvent `Cell`'s safety checks.
    ///
    /// This function is `unsafe` because `UnsafeCell`'s field is public.
    #[inline]
    #[unstable]
    pub unsafe fn as_unsafe_cell<'a>(&'a self) -> &'a UnsafeCell<T> {
        &self.value
    }
}

#[stable]
unsafe impl<T> Send for Cell<T> where T: Send {}

#[stable]
impl<T:Copy> Clone for Cell<T> {
    fn clone(&self) -> Cell<T> {
        Cell::new(self.get())
    }
}

#[stable]
impl<T:Default + Copy> Default for Cell<T> {
    #[stable]
    fn default() -> Cell<T> {
        Cell::new(Default::default())
    }
}

#[stable]
impl<T:PartialEq + Copy> PartialEq for Cell<T> {
    fn eq(&self, other: &Cell<T>) -> bool {
        self.get() == other.get()
    }
}

/// A mutable memory location with dynamically checked borrow rules
#[stable]
pub struct RefCell<T> {
    value: UnsafeCell<T>,
    borrow: Cell<BorrowFlag>,
}

// Values [1, MAX-1] represent the number of `Ref` active
// (will not outgrow its range since `uint` is the size of the address space)
type BorrowFlag = uint;
const UNUSED: BorrowFlag = 0;
const WRITING: BorrowFlag = -1;

impl<T> RefCell<T> {
    /// Create a new `RefCell` containing `value`
    #[stable]
    pub fn new(value: T) -> RefCell<T> {
        RefCell {
            value: UnsafeCell::new(value),
            borrow: Cell::new(UNUSED),
        }
    }

    /// Consumes the `RefCell`, returning the wrapped value.
    #[stable]
    pub fn into_inner(self) -> T {
        // Since this function takes `self` (the `RefCell`) by value, the
        // compiler statically verifies that it is not currently borrowed.
        // Therefore the following assertion is just a `debug_assert!`.
        debug_assert!(self.borrow.get() == UNUSED);
        unsafe { self.value.into_inner() }
    }

    /// Attempts to immutably borrow the wrapped value.
    ///
    /// The borrow lasts until the returned `Ref` exits scope. Multiple
    /// immutable borrows can be taken out at the same time.
    ///
    /// Returns `None` if the value is currently mutably borrowed.
    #[unstable = "may be renamed or removed"]
    pub fn try_borrow<'a>(&'a self) -> Option<Ref<'a, T>> {
        match BorrowRef::new(&self.borrow) {
            Some(b) => Some(Ref { _value: unsafe { &*self.value.get() }, _borrow: b }),
            None => None,
        }
    }

    /// Immutably borrows the wrapped value.
    ///
    /// The borrow lasts until the returned `Ref` exits scope. Multiple
    /// immutable borrows can be taken out at the same time.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently mutably borrowed.
    #[stable]
    pub fn borrow<'a>(&'a self) -> Ref<'a, T> {
        match self.try_borrow() {
            Some(ptr) => ptr,
            None => panic!("RefCell<T> already mutably borrowed")
        }
    }

    /// Mutably borrows the wrapped value.
    ///
    /// The borrow lasts until the returned `RefMut` exits scope. The value
    /// cannot be borrowed while this borrow is active.
    ///
    /// Returns `None` if the value is currently borrowed.
    #[unstable = "may be renamed or removed"]
    pub fn try_borrow_mut<'a>(&'a self) -> Option<RefMut<'a, T>> {
        match BorrowRefMut::new(&self.borrow) {
            Some(b) => Some(RefMut { _value: unsafe { &mut *self.value.get() }, _borrow: b }),
            None => None,
        }
    }

    /// Mutably borrows the wrapped value.
    ///
    /// The borrow lasts until the returned `RefMut` exits scope. The value
    /// cannot be borrowed while this borrow is active.
    ///
    /// # Panics
    ///
    /// Panics if the value is currently borrowed.
    #[stable]
    pub fn borrow_mut<'a>(&'a self) -> RefMut<'a, T> {
        match self.try_borrow_mut() {
            Some(ptr) => ptr,
            None => panic!("RefCell<T> already borrowed")
        }
    }

    /// Get a reference to the underlying `UnsafeCell`.
    ///
    /// This can be used to circumvent `RefCell`'s safety checks.
    ///
    /// This function is `unsafe` because `UnsafeCell`'s field is public.
    #[inline]
    #[unstable]
    pub unsafe fn as_unsafe_cell<'a>(&'a self) -> &'a UnsafeCell<T> {
        &self.value
    }
}

#[stable]
unsafe impl<T> Send for RefCell<T> where T: Send {}

#[stable]
impl<T: Clone> Clone for RefCell<T> {
    fn clone(&self) -> RefCell<T> {
        RefCell::new(self.borrow().clone())
    }
}

#[stable]
impl<T:Default> Default for RefCell<T> {
    #[stable]
    fn default() -> RefCell<T> {
        RefCell::new(Default::default())
    }
}

#[stable]
impl<T: PartialEq> PartialEq for RefCell<T> {
    fn eq(&self, other: &RefCell<T>) -> bool {
        *self.borrow() == *other.borrow()
    }
}

struct BorrowRef<'b> {
    _borrow: &'b Cell<BorrowFlag>,
}

impl<'b> BorrowRef<'b> {
    fn new(borrow: &'b Cell<BorrowFlag>) -> Option<BorrowRef<'b>> {
        match borrow.get() {
            WRITING => None,
            b => {
                borrow.set(b + 1);
                Some(BorrowRef { _borrow: borrow })
            },
        }
    }
}

#[unsafe_destructor]
impl<'b> Drop for BorrowRef<'b> {
    fn drop(&mut self) {
        let borrow = self._borrow.get();
        debug_assert!(borrow != WRITING && borrow != UNUSED);
        self._borrow.set(borrow - 1);
    }
}

impl<'b> Clone for BorrowRef<'b> {
    fn clone(&self) -> BorrowRef<'b> {
        // Since this Ref exists, we know the borrow flag
        // is not set to WRITING.
        let borrow = self._borrow.get();
        debug_assert!(borrow != WRITING && borrow != UNUSED);
        self._borrow.set(borrow + 1);
        BorrowRef { _borrow: self._borrow }
    }
}

/// Wraps a borrowed reference to a value in a `RefCell` box.
#[stable]
pub struct Ref<'b, T:'b> {
    // FIXME #12808: strange name to try to avoid interfering with
    // field accesses of the contained type via Deref
    _value: &'b T,
    _borrow: BorrowRef<'b>,
}

#[stable]
impl<'b, T> Deref for Ref<'b, T> {
    type Target = T;

    #[inline]
    fn deref<'a>(&'a self) -> &'a T {
        self._value
    }
}

/// Copy a `Ref`.
///
/// The `RefCell` is already immutably borrowed, so this cannot fail.
///
/// A `Clone` implementation would interfere with the widespread
/// use of `r.borrow().clone()` to clone the contents of a `RefCell`.
#[unstable = "likely to be moved to a method, pending language changes"]
pub fn clone_ref<'b, T:Clone>(orig: &Ref<'b, T>) -> Ref<'b, T> {
    Ref {
        _value: orig._value,
        _borrow: orig._borrow.clone(),
    }
}

struct BorrowRefMut<'b> {
    _borrow: &'b Cell<BorrowFlag>,
}

#[unsafe_destructor]
impl<'b> Drop for BorrowRefMut<'b> {
    fn drop(&mut self) {
        let borrow = self._borrow.get();
        debug_assert!(borrow == WRITING);
        self._borrow.set(UNUSED);
    }
}

impl<'b> BorrowRefMut<'b> {
    fn new(borrow: &'b Cell<BorrowFlag>) -> Option<BorrowRefMut<'b>> {
        match borrow.get() {
            UNUSED => {
                borrow.set(WRITING);
                Some(BorrowRefMut { _borrow: borrow })
            },
            _ => None,
        }
    }
}

/// Wraps a mutable borrowed reference to a value in a `RefCell` box.
#[stable]
pub struct RefMut<'b, T:'b> {
    // FIXME #12808: strange name to try to avoid interfering with
    // field accesses of the contained type via Deref
    _value: &'b mut T,
    _borrow: BorrowRefMut<'b>,
}

#[stable]
impl<'b, T> Deref for RefMut<'b, T> {
    type Target = T;

    #[inline]
    fn deref<'a>(&'a self) -> &'a T {
        self._value
    }
}

#[stable]
impl<'b, T> DerefMut for RefMut<'b, T> {
    #[inline]
    fn deref_mut<'a>(&'a mut self) -> &'a mut T {
        self._value
    }
}

/// The core primitive for interior mutability in Rust.
///
/// `UnsafeCell` type that wraps a type T and indicates unsafe interior
/// operations on the wrapped type. Types with an `UnsafeCell<T>` field are
/// considered to have an *unsafe interior*. The `UnsafeCell` type is the only
/// legal way to obtain aliasable data that is considered mutable. In general,
/// transmuting an &T type into an &mut T is considered undefined behavior.
///
/// Although it is possible to put an `UnsafeCell<T>` into static item, it is
/// not permitted to take the address of the static item if the item is not
/// declared as mutable. This rule exists because immutable static items are
/// stored in read-only memory, and thus any attempt to mutate their interior
/// can cause segfaults. Immutable static items containing `UnsafeCell<T>`
/// instances are still useful as read-only initializers, however, so we do not
/// forbid them altogether.
///
/// Types like `Cell` and `RefCell` use this type to wrap their internal data.
///
/// `UnsafeCell` doesn't opt-out from any kind, instead, types with an
/// `UnsafeCell` interior are expected to opt-out from kinds themselves.
///
/// # Example:
///
/// ```rust
/// use std::cell::UnsafeCell;
/// use std::marker::Sync;
///
/// struct NotThreadSafe<T> {
///     value: UnsafeCell<T>,
/// }
///
/// unsafe impl<T> Sync for NotThreadSafe<T> {}
/// ```
///
/// **NOTE:** `UnsafeCell<T>` fields are public to allow static initializers. It
/// is not recommended to access its fields directly, `get` should be used
/// instead.
#[lang="unsafe"]
#[stable]
pub struct UnsafeCell<T> {
    /// Wrapped value
    ///
    /// This field should not be accessed directly, it is made public for static
    /// initializers.
    #[unstable]
    pub value: T,
}

impl<T> UnsafeCell<T> {
    /// Construct a new instance of `UnsafeCell` which will wrap the specified
    /// value.
    ///
    /// All access to the inner value through methods is `unsafe`, and it is
    /// highly discouraged to access the fields directly.
    #[stable]
    pub fn new(value: T) -> UnsafeCell<T> {
        UnsafeCell { value: value }
    }

    /// Gets a mutable pointer to the wrapped value.
    #[inline]
    #[stable]
    pub fn get(&self) -> *mut T { &self.value as *const T as *mut T }

    /// Unwraps the value
    ///
    /// This function is unsafe because there is no guarantee that this or other
    /// tasks are currently inspecting the inner value.
    #[inline]
    #[stable]
    pub unsafe fn into_inner(self) -> T { self.value }
}
