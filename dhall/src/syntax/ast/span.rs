use std::rc::Rc;

/// A location in the source text
#[derive(Debug, Clone)]
pub struct ParsedSpan {
    input: Rc<str>,
    /// # Safety
    ///
    /// Must be a valid character boundary index into `input`.
    start: usize,
    /// # Safety
    ///
    /// Must be a valid character boundary index into `input`.
    end: usize,
}

#[derive(Debug, Clone)]
pub enum Span {
    /// A location in the source text
    Parsed(ParsedSpan),
    /// Desugarings
    DuplicateRecordFieldsSugar(Box<Span>, Box<Span>),
    DottedFieldSugar,
    RecordPunSugar,
    /// For expressions obtained from decoding binary
    Decoded,
    /// For expressions constructed during normalization/typecheck
    Artificial,
}

impl ParsedSpan {
    #[must_use]
    pub fn to_input(&self) -> String {
        self.input.to_string()
    }
    /// Convert to a byte range for consumption by `annotate_snippets`.
    #[must_use]
    pub fn as_byte_range(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

impl Span {
    #[must_use]
    pub fn make(input: Rc<str>, sp: pest::Span) -> Self {
        Span::Parsed(ParsedSpan {
            input,
            start: sp.start(),
            end: sp.end(),
        })
    }

    /// Takes the union of the two spans, i.e. the range of input covered by the two spans plus any
    /// input between them. Assumes that the spans come from the same input. Fails if one of the
    /// spans does not point to an input location.
    #[must_use]
    pub fn union(&self, other: &Span) -> Self {
        use Span::*;
        use std::cmp::{max, min};
        match (self, other) {
            (Parsed(x), Parsed(y)) if Rc::ptr_eq(&x.input, &y.input) => {
                Parsed(ParsedSpan {
                    input: x.input.clone(),
                    start: min(x.start, y.start),
                    end: max(x.end, y.end),
                })
            }
            (Parsed(_), Parsed(_)) => panic!(
                "Tried to union incompatible spans: {self:?} and {other:?}"
            ),
            (Parsed(x), _) | (_, Parsed(x)) => Parsed(x.clone()),
            _ => panic!(
                "Tried to union incompatible spans: {self:?} and {other:?}"
            ),
        }
    }
}
