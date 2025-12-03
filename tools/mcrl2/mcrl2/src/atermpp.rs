use std::fmt;
use std::marker::PhantomData;

use mcrl2_sys::atermpp::ffi::aterm;
use mcrl2_sys::atermpp::ffi::aterm_list;
use mcrl2_sys::atermpp::ffi::aterm_string;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_list_front;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_list_tail;
use mcrl2_sys::atermpp::ffi::mcrl2_aterm_to_string;
use mcrl2_sys::cxx::UniquePtr;

/// Represents a atermpp::aterm from the mCRL2 toolset.
pub struct ATerm {
    term: UniquePtr<aterm>,
}

impl ATerm {
    /// Creates a new `Mcrl2AtermList` from the given term.
    pub(crate) fn new(term: UniquePtr<aterm>) -> Self {
        Self { term }
    }

    /// Returns a reference to the underlying term.
    pub fn get(&self) -> &aterm {
        self.term.as_ref().expect("ATerm is null")
    }

    /// Casts the underlying term to the given type.
    pub fn cast<T>(self) -> UniquePtr<T> {
        std::mem::transmute(self.term)
    }
}

impl fmt::Debug for ATerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mcrl2ATerm {}", mcrl2_aterm_to_string(&self.term).unwrap())
    }
}

/// Represents a atermpp::aterm_string from the mCRL2 toolset.
pub struct ATermString {
    term: UniquePtr<aterm_string>,
}

impl ATermString {
    /// Creates a new `ATermString` from the given term.
    pub(crate) fn new(term: UniquePtr<aterm_string>) -> Self {
        Self { term }
    }
}

impl fmt::Debug for ATermString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", mcrl2_aterm_string_to_string(&self.term).unwrap())
    }
}

/// Represents a list of terms from the mCRL2 toolset.
#[derive(Clone)]
pub struct AtermList<T> {
    term: UniquePtr<aterm_list>,
    _marker: PhantomData<T>,
}

impl<T> AtermList<T> {
    /// Returns the length of the list.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the head of the list
    pub fn head(&self) -> T 
        where T: From<ATerm>
    {
        ATerm::new(mcrl2_aterm_list_front(&self.term.get())).into()
    }

    /// Returns the tail of the list
    pub fn tail(&self) -> AtermList<T> {
        AtermList::new(ATerm::new(mcrl2_aterm_list_tail(&self.term.get()).into()))
    }

    /// Returns an iterator over the elements of the list.
    pub fn iter(&self) -> ATermListIter<T> {
        ATermListIter { list: self.clone() }
    }

    /// Converts the list to a `Vec<T>`.
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().collect()
    }

    /// Creates a new list from the given term.
    pub(crate) fn new(term: UniquePtr<aterm_list>) -> Self
    {
        AtermList {
            term,
            _marker: PhantomData,
        }
    }
}

impl From<ATerm> for AtermList<ATerm> {
    fn from(term: ATerm) -> Self {
        AtermList::new(term)
    }
}

pub struct ATermListIter<T> {
    list: AtermList<T>,
}

impl Iterator for ATermListIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {}
}
