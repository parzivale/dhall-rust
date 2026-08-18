use annotate_snippets::{
    renderer::DecorStyle, AnnotationKind, Element, Level, Renderer, Snippet,
};

use crate::syntax::{ParsedSpan, Span};

/// How severe an annotation is.
///
/// This mirrors annotate-snippets' levels rather than re-exporting them, so
/// that this crate's public API does not shift every time that dependency
/// reshuffles its types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationType {
    Error,
    Help,
    Note,
}

impl AnnotationType {
    fn to_level(self) -> Level<'static> {
        match self {
            AnnotationType::Error => Level::ERROR,
            AnnotationType::Help => Level::HELP,
            AnnotationType::Note => Level::NOTE,
        }
    }

    /// annotate-snippets no longer gives each in-source annotation its own
    /// level; they are either the primary span or supporting context.
    fn to_annotation_kind(self) -> AnnotationKind {
        match self {
            AnnotationType::Error => AnnotationKind::Primary,
            AnnotationType::Help | AnnotationType::Note => {
                AnnotationKind::Context
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ErrorBuilder {
    title: FreeAnnotation,
    annotations: Vec<SpannedAnnotation>,
    footer: Vec<FreeAnnotation>,
    /// Inducate that the current builder has already been consumed and consuming it again should
    /// panic.
    consumed: bool,
}

#[derive(Debug, Clone)]
struct SpannedAnnotation {
    span: ParsedSpan,
    message: String,
    annotation_type: AnnotationType,
}

#[derive(Debug, Clone)]
struct FreeAnnotation {
    message: String,
    annotation_type: AnnotationType,
}

/// A builder that uses the annotate_snippets library to display nice error messages about source
/// code locations.
impl ErrorBuilder {
    pub fn new(message: impl ToString) -> Self {
        ErrorBuilder {
            title: FreeAnnotation {
                message: message.to_string(),
                annotation_type: AnnotationType::Error,
            },
            annotations: Vec::new(),
            footer: Vec::new(),
            consumed: false,
        }
    }

    pub fn span_annot(
        &mut self,
        span: Span,
        message: impl ToString,
        annotation_type: AnnotationType,
    ) -> &mut Self {
        // Ignore spans not coming from a source file
        let span = match span {
            Span::Parsed(span) => span,
            _ => return self,
        };
        self.annotations.push(SpannedAnnotation {
            span,
            message: message.to_string(),
            annotation_type,
        });
        self
    }
    pub fn footer_annot(
        &mut self,
        message: impl ToString,
        annotation_type: AnnotationType,
    ) -> &mut Self {
        self.footer.push(FreeAnnotation {
            message: message.to_string(),
            annotation_type,
        });
        self
    }

    pub fn span_err(
        &mut self,
        span: Span,
        message: impl ToString,
    ) -> &mut Self {
        self.span_annot(span, message, AnnotationType::Error)
    }
    pub fn span_help(
        &mut self,
        span: Span,
        message: impl ToString,
    ) -> &mut Self {
        self.span_annot(span, message, AnnotationType::Help)
    }
    pub fn help(&mut self, message: impl ToString) -> &mut Self {
        self.footer_annot(message, AnnotationType::Help)
    }
    pub fn note(&mut self, message: impl ToString) -> &mut Self {
        self.footer_annot(message, AnnotationType::Note)
    }

    // TODO: handle multiple files
    pub fn format(&mut self) -> String {
        if self.consumed {
            panic!("tried to format the same ErrorBuilder twice")
        }
        let this = std::mem::take(self);
        self.consumed = true;

        let input;
        let mut elements: Vec<Element<'_>> = Vec::new();

        if !this.annotations.is_empty() {
            input = this.annotations[0].span.to_input();
            let mut snippet = Snippet::source(input.as_str())
                .line_start(1) // TODO
                .path("<current file>")
                .fold(true);
            for annot in &this.annotations {
                snippet = snippet.annotation(
                    annot
                        .annotation_type
                        .to_annotation_kind()
                        .span(annot.span.as_byte_range())
                        .label(annot.message.as_str()),
                );
            }
            elements.push(snippet.into());
        }

        for annot in &this.footer {
            elements.push(
                annot
                    .annotation_type
                    .to_level()
                    .message(annot.message.as_str())
                    .into(),
            );
        }

        let group = this
            .title
            .annotation_type
            .to_level()
            .primary_title(this.title.message.as_str())
            .elements(elements);

        // Ascii decor and no colour, matching what this crate rendered before
        // annotate-snippets switched its default to styled Unicode.
        Renderer::plain()
            .decor_style(DecorStyle::Ascii)
            .render(&[group])
    }
}

impl Default for FreeAnnotation {
    fn default() -> Self {
        FreeAnnotation {
            message: String::new(),
            annotation_type: AnnotationType::Error,
        }
    }
}
