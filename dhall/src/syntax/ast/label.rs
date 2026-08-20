use smol_str::SmolStr;

// The type for labels throughout the AST
// It owns the data because otherwise lifetimes would make recursive imports impossible
//
// `SmolStr` rather than `Rc<str>`: labels are identifiers, so they are nearly
// always within its 23-byte inline capacity and cost no allocation and no
// refcount at all. Anything longer falls back to an `Arc<str>`, which keeps the
// whole AST `Send` and `Sync` -- an `Expr` can then be held across threads,
// which an `Rc` ruled out.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label(SmolStr);

impl From<String> for Label {
    fn from(s: String) -> Self {
        Label(s.into())
    }
}

impl<'a> From<&'a str> for Label {
    fn from(s: &'a str) -> Self {
        Label(s.into())
    }
}

impl From<&Label> for String {
    fn from(x: &Label) -> String {
        x.0.as_str().to_owned()
    }
}

impl std::borrow::Borrow<str> for Label {
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}

impl Label {
    /// Builds a label from a string literal without allocating, whatever its
    /// length.
    #[must_use]
    pub const fn from_static(s: &'static str) -> Label {
        Label(SmolStr::new_static(s))
    }
    #[must_use]
    pub fn from_str(s: &str) -> Label {
        Label(s.into())
    }
    #[must_use]
    pub fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}
