use std::io::Error as IOError;

use crate::semantics::resolve::{CyclesStack, ImportLocation};
use crate::syntax::{FilePrefix, Import, ImportTarget, ParseError};

mod builder;
pub use builder::*;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    /// Whether the `?` operator may recover from this error.
    ///
    /// Carried alongside the kind because a failed import is re-wrapped as a
    /// type error for display, which discards what actually went wrong.
    recoverable: bool,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    IO(IOError),
    Parse(ParseError),
    Decode(DecodeError),
    Encode(EncodeError),
    Resolve(ImportError),
    Typecheck(TypeError),
    Cache(CacheError),
}

#[derive(Debug)]
pub enum ImportError {
    Missing,
    MissingEnvVar,
    MissingHome,
    SanityCheck,
    /// A cross-origin remote import did not opt in via
    /// `Access-Control-Allow-Origin`.
    CorsRejected {
        parent: String,
        child: String,
    },
    /// A remote import was reached while remote imports were disabled.
    RemoteImportsDisabled {
        location: String,
    },
    UnexpectedImport(Import<()>),
    ImportCycle(CyclesStack, ImportLocation),
    Url(url::ParseError),
}

#[derive(Debug)]
pub enum DecodeError {
    CBORError(minicbor::decode::Error),
    WrongFormatError(String),
}

#[derive(Debug)]
pub enum EncodeError {
    CBORError(minicbor::encode::Error<core::convert::Infallible>),
}

/// A structured type error
#[derive(Debug)]
pub struct TypeError {
    message: TypeMessage,
}

/// The specific type error
#[derive(Debug)]
pub enum TypeMessage {
    Custom(String),
}

#[derive(Debug)]
pub enum CacheError {
    MissingConfiguration,
    InitialisationError { cause: IOError },
    CacheHashInvalid,
}

impl Error {
    pub fn new(kind: ErrorKind) -> Self {
        // Only an import that could not be *retrieved* is recoverable. One that
        // was retrieved but does not parse or typecheck, a cyclic import, and a
        // failed integrity check must all propagate, or `?` would silently
        // paper over a corrupt or malicious dependency.
        let recoverable = matches!(
            kind,
            ErrorKind::IO(_)
                | ErrorKind::Resolve(
                    ImportError::Missing
                        | ImportError::MissingEnvVar
                        | ImportError::MissingHome
                )
        );
        Error { kind, recoverable }
    }
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }
    /// Whether the `?` operator may recover from this error.
    pub fn is_recoverable(&self) -> bool {
        self.recoverable
    }
    /// Take this error's recoverability from `original`.
    ///
    /// For re-wrapping a failure in a more informative error without deciding
    /// afresh whether `?` may swallow it.
    pub fn inheriting_recoverability(mut self, original: &Error) -> Self {
        self.recoverable = original.recoverable;
        self
    }
}

impl TypeError {
    pub fn new(message: TypeMessage) -> Self {
        TypeError { message }
    }
}

/// Render an import target for an error message.
///
/// `Import<()>` cannot use the `Display` impl in the printer, which needs
/// `SubExpr: Display` for the `using` clause, and `()` is not.
fn fmt_import_target(target: &ImportTarget<()>) -> String {
    match target {
        ImportTarget::Local(prefix, path) => {
            let prefix = match prefix {
                FilePrefix::Here => ".",
                FilePrefix::Parent => "..",
                FilePrefix::Home => "~",
                FilePrefix::Absolute => "",
            };
            format!("{}/{}", prefix, path.file_path.join("/"))
        }
        ImportTarget::Remote(url) => {
            format!(
                "{}://{}/{}",
                url.scheme,
                url.authority,
                url.path.file_path.join("/")
            )
        }
        ImportTarget::Env(name) => format!("env:{}", name),
        ImportTarget::Missing => "missing".to_string(),
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ImportError::Missing => {
                write!(f, "the `missing` import never resolves")
            }
            ImportError::MissingEnvVar => {
                write!(f, "the environment variable is not set")
            }
            ImportError::MissingHome => {
                write!(f, "could not locate the home directory")
            }
            ImportError::SanityCheck => write!(
                f,
                "a remote import may not depend on an environment variable or \
                 a local file"
            ),
            ImportError::CorsRejected { parent, child } => write!(
                f,
                "{} does not grant {} access via Access-Control-Allow-Origin",
                child, parent
            ),
            ImportError::RemoteImportsDisabled { location } => {
                write!(f, "remote imports are disabled: {}", location)
            }
            ImportError::UnexpectedImport(import) => write!(
                f,
                "importing `{}` is not allowed here",
                fmt_import_target(&import.location)
            ),
            // Innermost first, which is the order they were entered in.
            ImportError::ImportCycle(stack, location) => {
                write!(
                    f,
                    "cyclic import: {} is already being resolved",
                    location
                )?;
                for entry in stack.iter().rev() {
                    write!(f, "\n  ... which was imported by {}", entry)?;
                }
                Ok(())
            }
            ImportError::Url(err) => write!(f, "invalid import URL: {}", err),
        }
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use TypeMessage::*;
        let msg = match &self.message {
            Custom(s) => format!("Type error: {}", s),
        };
        write!(f, "{}", msg)
    }
}

impl std::error::Error for TypeError {}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let msg = match self {
            EncodeError::CBORError(e) => format!("Encode error: {}", e),
        };
        write!(f, "{}", msg)
    }
}

impl std::error::Error for EncodeError {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.kind {
            ErrorKind::IO(err) => write!(f, "{}", err),
            ErrorKind::Parse(err) => write!(f, "{}", err),
            ErrorKind::Decode(err) => write!(f, "{:?}", err),
            ErrorKind::Encode(err) => write!(f, "{:?}", err),
            ErrorKind::Resolve(err) => write!(f, "{}", err),
            ErrorKind::Typecheck(err) => write!(f, "{}", err),
            ErrorKind::Cache(err) => write!(f, "{:?}", err),
        }
    }
}

impl std::error::Error for Error {}
impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error::new(kind)
    }
}
impl From<IOError> for Error {
    fn from(err: IOError) -> Error {
        ErrorKind::IO(err).into()
    }
}
impl From<ParseError> for Error {
    fn from(err: ParseError) -> Error {
        ErrorKind::Parse(err).into()
    }
}
impl From<url::ParseError> for Error {
    fn from(err: url::ParseError) -> Error {
        ErrorKind::Resolve(ImportError::Url(err)).into()
    }
}
impl From<DecodeError> for Error {
    fn from(err: DecodeError) -> Error {
        ErrorKind::Decode(err).into()
    }
}
impl From<EncodeError> for Error {
    fn from(err: EncodeError) -> Error {
        ErrorKind::Encode(err).into()
    }
}
impl From<ImportError> for Error {
    fn from(err: ImportError) -> Error {
        ErrorKind::Resolve(err).into()
    }
}
impl From<TypeError> for Error {
    fn from(err: TypeError) -> Error {
        ErrorKind::Typecheck(err).into()
    }
}
impl From<CacheError> for Error {
    fn from(err: CacheError) -> Error {
        ErrorKind::Cache(err).into()
    }
}
