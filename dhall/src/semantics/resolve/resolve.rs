use itertools::Itertools;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use url::Url;

use crate::builtins::Builtin;
use crate::error::ErrorBuilder;
use crate::error::{Error, ImportError};
use crate::operations::{BinOp, OpKind};
use crate::semantics::{
    Hir, HirKind, ImportEnv, NameEnv, Nir, NirKind, Type, mkerr,
};
use crate::syntax;
use crate::syntax::{
    Expr, ExprKind, FilePath, FilePrefix, Hash, ImportMode, ImportTarget,
    Label, Span, URL, UnspannedExpr,
};
use crate::{
    Ctxt, ImportAlternativeId, ImportId, ImportResultId, Parsed, Resolved,
    Typed,
};

// TODO: evaluate import headers
pub type Import = syntax::Import<()>;

/// The location of some data, usually some dhall code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ImportLocationKind {
    /// Local file
    Local(PathBuf),
    /// Remote file
    Remote(Url),
    /// Environment variable
    Env(String),
    /// Data without a location; chaining will start from current directory.
    Missing,
    /// Token to signal that thi sfile should contain no imports.
    NoImport,
}

/// The location of some data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImportLocation {
    kind: ImportLocationKind,
    mode: ImportMode,
    /// Custom headers to send, from a `using` clause. May have been inherited
    /// from the parent import; see `chain`.
    ///
    /// Part of the identity of a location because the response can depend on
    /// them, so two otherwise-identical imports with different headers must not
    /// share a cache entry.
    headers: Vec<(String, String)>,
}

impl ImportLocationKind {
    fn chain_local(
        &self,
        prefix: FilePrefix,
        path: &FilePath,
    ) -> Result<Self, Error> {
        Ok(match self {
            ImportLocationKind::Local(..)
            | ImportLocationKind::Env(..)
            | ImportLocationKind::Missing => {
                let dir = match self {
                    ImportLocationKind::Local(path) => {
                        path.parent().unwrap().to_owned()
                    }
                    ImportLocationKind::Env(..)
                    | ImportLocationKind::Missing => std::env::current_dir()?,
                    _ => unreachable!(),
                };
                let mut dir: Vec<String> = dir
                    .components()
                    .map(|component| {
                        component.as_os_str().to_string_lossy().into_owned()
                    })
                    .collect();
                let root = match prefix {
                    FilePrefix::Here => dir,
                    FilePrefix::Parent => {
                        dir.push("..".to_string());
                        dir
                    }
                    FilePrefix::Absolute | FilePrefix::Home => vec![],
                };
                let path: Vec<_> = root
                    .into_iter()
                    .chain(path.file_path.iter().cloned())
                    .collect();
                let path =
                    (FilePath { file_path: path }).canonicalize().file_path;
                let prefix = match prefix {
                    FilePrefix::Here | FilePrefix::Parent => ".",
                    FilePrefix::Absolute => "/",
                    FilePrefix::Home => "~",
                };
                let path =
                    Some(prefix.to_string()).into_iter().chain(path).collect();
                ImportLocationKind::Local(path)
            }
            ImportLocationKind::Remote(url) => {
                let mut url = url.clone();
                match prefix {
                    FilePrefix::Here => {}
                    FilePrefix::Parent => {
                        url = url.join("..")?;
                    }
                    FilePrefix::Absolute | FilePrefix::Home => {
                        panic!("error")
                    }
                }
                url = url.join(&path.file_path.join("/"))?;
                ImportLocationKind::Remote(url)
            }
            ImportLocationKind::NoImport => unreachable!(),
        })
    }

    fn fetch_dhall(
        &self,
        parent: &ImportLocationKind,
        headers: &[(String, String)],
    ) -> Result<Parsed, Error> {
        Ok(match self {
            ImportLocationKind::Local(path) => Parsed::parse_file(path)?,
            ImportLocationKind::Remote(url) => {
                let response = download_http(url.clone(), headers)?;
                check_cors(parent, url, &response)?;
                crate::semantics::parse::parse_remote_body(
                    url.clone(),
                    &response.body,
                    headers.to_vec(),
                )?
            }
            ImportLocationKind::Env(var_name) => {
                let Ok(val) = env::var(var_name) else {
                    return Err(ImportError::MissingEnvVar.into());
                };
                Parsed::parse_str(&val)?
            }
            ImportLocationKind::Missing => {
                return Err(ImportError::Missing.into());
            }
            ImportLocationKind::NoImport => unreachable!(),
        })
    }

    fn fetch_text(
        &self,
        parent: &ImportLocationKind,
        headers: &[(String, String)],
    ) -> Result<String, Error> {
        Ok(match self {
            ImportLocationKind::Local(path) => {
                let path = resolve_home(path)?;
                std::fs::read_to_string(path)?
            }
            ImportLocationKind::Remote(url) => {
                let response = download_http(url.clone(), headers)?;
                check_cors(parent, url, &response)?;
                response.body
            }
            ImportLocationKind::Env(var_name) => match env::var(var_name) {
                Ok(val) => val,
                Err(_) => return Err(ImportError::MissingEnvVar.into()),
            },
            ImportLocationKind::Missing => {
                return Err(ImportError::Missing.into());
            }
            ImportLocationKind::NoImport => unreachable!(),
        })
    }

    fn to_location(&self) -> Expr {
        let (field_name, arg) = match self {
            ImportLocationKind::Local(path) => {
                ("Local", Some(path.to_string_lossy().into_owned()))
            }
            ImportLocationKind::Remote(url) => {
                ("Remote", Some(url.to_string()))
            }
            ImportLocationKind::Env(name) => {
                ("Environment", Some(name.clone()))
            }
            ImportLocationKind::Missing => ("Missing", None),
            ImportLocationKind::NoImport => unreachable!(),
        };

        let asloc_ty = make_aslocation_uniontype();
        let expr =
            mkexpr(ExprKind::Op(OpKind::Field(asloc_ty, field_name.into())));
        match arg {
            Some(arg) => mkexpr(ExprKind::Op(OpKind::App(
                expr,
                mkexpr(ExprKind::TextLit(arg.into())),
            ))),
            None => expr,
        }
    }
}

impl std::fmt::Display for ImportLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.kind {
            ImportLocationKind::Local(path) => write!(f, "{}", path.display()),
            ImportLocationKind::Remote(url) => write!(f, "{url}"),
            ImportLocationKind::Env(name) => write!(f, "env:{name}"),
            ImportLocationKind::Missing => write!(f, "missing"),
            ImportLocationKind::NoImport => write!(f, "<no import>"),
        }
    }
}

impl ImportLocation {
    #[must_use]
    pub fn dhall_code_of_unknown_origin() -> Self {
        ImportLocation {
            kind: ImportLocationKind::Missing,
            mode: ImportMode::Code,
            headers: Vec::new(),
        }
    }
    #[must_use]
    pub fn dhall_code_without_imports() -> Self {
        ImportLocation {
            kind: ImportLocationKind::NoImport,
            mode: ImportMode::Code,
            headers: Vec::new(),
        }
    }
    #[must_use]
    pub fn local_dhall_code(path: PathBuf) -> Self {
        ImportLocation {
            kind: ImportLocationKind::Local(path),
            mode: ImportMode::Code,
            headers: Vec::new(),
        }
    }
    #[must_use]
    pub fn remote_dhall_code(url: Url) -> Self {
        Self::remote_dhall_code_using(url, Vec::new())
    }
    /// As [`remote_dhall_code`], carrying the headers the file was fetched
    /// with.
    ///
    /// They have to survive onto the location of the *fetched* expression, or
    /// its relative imports have no parent headers to inherit and forwarding
    /// stops after one hop.
    #[must_use]
    pub fn remote_dhall_code_using(
        url: Url,
        headers: Vec<(String, String)>,
    ) -> Self {
        ImportLocation {
            kind: ImportLocationKind::Remote(url),
            mode: ImportMode::Code,
            headers,
        }
    }

    /// Given an import pointing to `target` found in the current location, compute the next
    /// location, or error if not allowed.
    /// `sanity_check` indicates whether to check if that location is allowed to be referenced,
    /// for example to prevent a remote file from reading an environment variable.
    fn chain(
        &self,
        import: &Import,
        headers: Vec<(String, String)>,
    ) -> Result<ImportLocation, Error> {
        // Makes no sense to chain an import if the current file is not a dhall file.
        assert!(matches!(self.mode, ImportMode::Code));
        if matches!(self.kind, ImportLocationKind::NoImport) {
            Err(ImportError::UnexpectedImport(import.clone()))?;
        }

        let kind = match &import.location {
            ImportTarget::Local(prefix, path) => {
                self.kind.chain_local(*prefix, path)?
            }
            ImportTarget::Remote(remote) => {
                // A remote parent reaching another origin is allowed here and
                // checked once the response is in hand: the decision needs the
                // child's `Access-Control-Allow-Origin` header. See check_cors.
                let mut url = Url::parse(&format!(
                    "{}://{}",
                    remote.scheme, remote.authority
                ))?;
                url.set_path(&remote.path.file_path.iter().join("/"));
                url.set_query(remote.query.as_ref().map(String::as_ref));
                ImportLocationKind::Remote(url)
            }
            ImportTarget::Env(var_name) => {
                if matches!(self.kind, ImportLocationKind::Remote(..))
                    && !matches!(import.mode, ImportMode::Location)
                {
                    return Err(ImportError::SanityCheck.into());
                }
                ImportLocationKind::Env(var_name.clone())
            }
            ImportTarget::Missing => ImportLocationKind::Missing,
        };
        // Header forwarding, per standard/imports.md. A `using` clause on the
        // child always wins. Failing that, a *relative* child of a remote
        // parent inherits the parent's headers -- it is the same server, so the
        // credentials still apply. An absolute child does not, which is what
        // stops a token for one host being handed to another.
        let is_relative = matches!(
            import.location,
            ImportTarget::Local(FilePrefix::Here | FilePrefix::Parent, _)
        );
        let headers = if !headers.is_empty() {
            headers
        } else if is_relative
            && matches!(self.kind, ImportLocationKind::Remote(..))
        {
            self.headers.clone()
        } else {
            Vec::new()
        };

        Ok(ImportLocation {
            kind,
            mode: import.mode,
            headers,
        })
    }

    /// Fetches the expression corresponding to this location.
    fn fetch<'cx>(
        &self,
        env: &mut ImportEnv<'cx>,
        parent: &ImportLocationKind,
        span: Span,
    ) -> Result<Typed<'cx>, Error> {
        let cx = env.cx();
        let typed = match self.mode {
            ImportMode::Code => {
                let parsed = self.kind.fetch_dhall(parent, &self.headers)?;
                let typed = parsed.resolve_with_env(env)?.typecheck(cx)?;
                Typed {
                    // TODO: manage to keep the Nir around. Will need fixing variables.
                    hir: typed.normalize(cx).to_hir(),
                    ty: typed.ty,
                }
            }
            ImportMode::RawText => {
                let text = self.kind.fetch_text(parent, &self.headers)?;
                Typed {
                    hir: Hir::new(
                        HirKind::Expr(ExprKind::TextLit(text.into())),
                        span,
                    ),
                    ty: Type::from_builtin(cx, Builtin::Text),
                }
            }
            ImportMode::Location => {
                let expr = self.kind.to_location();
                Parsed::from_expr_without_imports(expr)
                    .resolve(cx)
                    .unwrap()
                    .typecheck(cx)
                    .unwrap()
            }
        };
        Ok(typed)
    }
}

fn mkexpr(kind: UnspannedExpr) -> Expr {
    Expr::new(kind, Span::Artificial)
}

/// Read a normalized `using` clause into the headers to send.
///
/// The standard accepts either field naming:
/// `List { mapKey : Text, mapValue : Text }` or
/// `List { header : Text, value : Text }`.
fn headers_from_nir(nir: &Nir<'_>) -> Vec<(String, String)> {
    // An empty list has nothing to send.
    let NirKind::NEListLit(entries) = nir.kind() else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let NirKind::RecordLit(fields) = entry.kind() else {
                return None;
            };
            let get = |names: [&str; 2]| {
                names.iter().find_map(|name| {
                    match fields.get(&Label::from(*name))?.kind() {
                        NirKind::TextLit(t) => t.as_text(),
                        _ => None,
                    }
                })
            };
            Some((get(["mapKey", "header"])?, get(["mapValue", "value"])?))
        })
        .collect()
}

/// The CORS judgment, `corsCompliant(parent, child, headers)` in
/// standard/imports.md.
///
/// Guards against server-side request forgery: without it, a remote file could
/// use whoever resolves it as a proxy onto hosts they can reach and it cannot.
/// A cross-origin remote import therefore has to opt in through a response
/// header, and anything other than exactly one matching value is a rejection.
fn check_cors(
    parent: &ImportLocationKind,
    child: &Url,
    response: &HttpResponse,
) -> Result<(), Error> {
    // Only a remote parent can forge a request; a local file already has the
    // user's authority.
    let ImportLocationKind::Remote(parent) = parent else {
        return Ok(());
    };

    // Same origin needs no header.
    if parent.scheme() == child.scheme()
        && parent.authority() == child.authority()
    {
        return Ok(());
    }

    let origin = format!("{}://{}", parent.scheme(), parent.authority());
    match response.allow_origin.as_slice() {
        [allowed] if allowed == "*" || *allowed == origin => Ok(()),
        _ => Err(ImportError::CorsRejected {
            parent: origin,
            child: child.to_string(),
        }
        .into()),
    }
}

/// What a remote import returned.
#[derive(Debug, Clone, Default)]
pub(crate) struct HttpResponse {
    pub body: String,
    /// Every value of the `Access-Control-Allow-Origin` response header.
    ///
    /// Kept as a list because the CORS judgment rejects a response carrying
    /// anything other than exactly one.
    pub allow_origin: Vec<String>,
}

// TODO: error handling
#[cfg(all(not(target_arch = "wasm32"), feature = "reqwest"))]
pub(crate) fn download_http(
    url: Url,
    headers: &[(String, String)],
) -> Result<HttpResponse, Error> {
    let mut request = reqwest::blocking::Client::new().get(url);
    for (name, value) in headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().unwrap();
    let allow_origin = response
        .headers()
        .get_all(reqwest::header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(str::to_owned)
        .collect();
    Ok(HttpResponse {
        body: response.text().unwrap(),
        allow_origin,
    })
}
#[cfg(all(not(target_arch = "wasm32"), not(feature = "reqwest")))]
pub(crate) fn download_http(
    _url: Url,
    _headers: &[(String, String)],
) -> Result<HttpResponse, Error> {
    panic!("Remote imports are disabled in this build of dhall-rust")
}
#[cfg(target_arch = "wasm32")]
pub(crate) fn download_http(
    _url: Url,
    _headers: &[(String, String)],
) -> Result<HttpResponse, Error> {
    panic!("Remote imports are not supported on wasm yet")
}

fn make_aslocation_uniontype() -> Expr {
    let text_type = mkexpr(ExprKind::Builtin(Builtin::Text));
    let mut union = BTreeMap::default();
    union.insert("Local".into(), Some(text_type.clone()));
    union.insert("Remote".into(), Some(text_type.clone()));
    union.insert("Environment".into(), Some(text_type));
    union.insert("Missing".into(), None);
    mkexpr(ExprKind::UnionType(union))
}

pub fn check_hash<'cx>(
    cx: Ctxt<'cx>,
    import: ImportId<'cx>,
    result: ImportResultId<'cx>,
) -> Result<(), Error> {
    let import = &cx[import];
    if let (ImportMode::Code, Some(Hash::SHA256(hash))) =
        (import.import.mode, &import.import.hash)
    {
        let expr = cx[result].hir.to_expr_alpha(cx);
        let actual_hash = expr.sha256_hash()?;
        if hash[..] != actual_hash[..] {
            mkerr(
                ErrorBuilder::new("hash mismatch")
                    .span_err(import.span.clone(), "hash mismatch")
                    .note(format!("Expected sha256:{}", hex::encode(hash)))
                    .note(format!(
                        "Found    sha256:{}",
                        hex::encode(actual_hash)
                    ))
                    .format(),
            )?;
        }
    }
    Ok(())
}

/// Desugar the first level of the expression.
fn desugar(expr: &Expr) -> Cow<'_, Expr> {
    match expr.kind() {
        ExprKind::Op(OpKind::Completion(ty, compl)) => {
            let ty_field_default = Expr::new(
                ExprKind::Op(OpKind::Field(ty.clone(), "default".into())),
                expr.span(),
            );
            let merged = Expr::new(
                ExprKind::Op(OpKind::BinOp(
                    BinOp::RightBiasedRecordMerge,
                    ty_field_default,
                    compl.clone(),
                )),
                expr.span(),
            );
            let ty_field_type = Expr::new(
                ExprKind::Op(OpKind::Field(ty.clone(), "Type".into())),
                expr.span(),
            );
            Cow::Owned(Expr::new(
                ExprKind::Annot(merged, ty_field_type),
                expr.span(),
            ))
        }
        _ => Cow::Borrowed(expr),
    }
}

/// Fetch the import and store the result in the global context.
fn fetch_import<'cx>(
    env: &mut ImportEnv<'cx>,
    import_id: ImportId<'cx>,
) -> Result<ImportResultId<'cx>, Error> {
    let cx = env.cx();
    let import = &cx[import_id].import;
    let span = cx[import_id].span.clone();

    // Resolve the `using [ ... ]` clause before fetching anything. It was
    // traversed in an empty context, so a header referring to a surrounding
    // binding is an unbound variable and fails to typecheck here.
    let headers = match &cx[import_id].headers {
        Some(headers) => {
            let typed = crate::semantics::typecheck(cx, headers)?;
            headers_from_nir(&typed.as_hir().eval_closed_expr(cx))
        }
        None => Vec::new(),
    };

    let location = cx[import_id].base_location.chain(import, headers)?;

    // Checked after chaining, because a relative import inside a remote file
    // chains to a remote location: what matters is where the request would
    // actually go, not how the import was written.
    if !env.allow_remote()
        && matches!(location.kind, ImportLocationKind::Remote(..))
    {
        return Err(ImportError::RemoteImportsDisabled {
            location: match &location.kind {
                ImportLocationKind::Remote(url) => url.to_string(),
                _ => unreachable!("checked to be remote just above"),
            },
        }
        .into());
    }

    // If the hash is in the on-disk cache, return
    // the cached contents.
    if let Some(typed) = env.get_from_disk_cache(&import.hash) {
        // No need to check the hash, it was checked before reading the file.
        // We also don't write to the in-memory cache, because the location might be completely
        // unrelated to the cached file (e.g. `missing sha256:...` is valid).
        // This actually means that importing many times a same hashed import will take
        // longer than importing many times a same non-hashed import.
        let res_id = cx.push_import_result(typed);
        return Ok(res_id);
    }

    // If the import is in the in-memory cache return the cached contents. Otherwise fetch the
    // import.
    let res_id = if let Some(res_id) = env.get_from_mem_cache(&location) {
        res_id
    } else {
        // Resolve this import, making sure that recursive imports don't cycle back to the
        // current one.
        let res = env.with_cycle_detection(location.clone(), |env| {
            location.fetch(env, &cx[import_id].base_location.kind, span.clone())
        });
        let typed = match res {
            Ok(typed) => typed,
            Err(e) => {
                // Wrapping the failure to point at the import site discards its
                // kind, so carry the recoverability decision across by hand;
                // `?` further out still needs it.
                let wrapped: Error = mkerr::<(), _>(
                    ErrorBuilder::new("error")
                        .span_err(span.clone(), e.to_string())
                        .format(),
                )
                .unwrap_err()
                .into();
                return Err(wrapped.inheriting_recoverability(&e));
            }
        };

        let res_id = cx.push_import_result(typed);
        // Cache the mapping from this location to the result.
        env.write_to_mem_cache(location, res_id);
        res_id
    };

    // Add the resolved import to the on-disk cache if the hash matches.
    env.check_hash(import_id, res_id)?;
    env.write_to_disk_cache(&import.hash, res_id);

    Ok(res_id)
}

/// Part of a tree of imports.
#[derive(Debug, Clone, Copy)]
pub enum ImportNode<'cx> {
    Import(ImportId<'cx>),
    Alternative(ImportAlternativeId<'cx>),
}

/// Traverse the expression and replace each import and import alternative by an id into the global
/// context. The ids are also accumulated into `nodes` so that we can resolve them afterwards.
fn traverse_accumulate<'cx>(
    env: &mut ImportEnv<'cx>,
    name_env: &mut NameEnv,
    nodes: &mut Vec<ImportNode<'cx>>,
    base_location: &ImportLocation,
    expr: &Expr,
) -> Hir<'cx> {
    let cx = env.cx();
    let expr = desugar(expr);
    let kind = match expr.kind() {
        ExprKind::Var(var) => match name_env.unlabel_var(var) {
            Some(v) => HirKind::Var(v),
            None => HirKind::MissingVar(var.clone()),
        },
        // Handled here rather than after the generic traversal below, because
        // the `using [ ... ]` clause must *not* be traversed in the enclosing
        // scope. The standard typechecks custom headers in an empty context:
        // resolution happens before normalization, and headers must not be able
        // to leak program state. So a header mentioning a surrounding `let`
        // resolves to an unbound variable, and fails to typecheck later.
        //
        // Headers are an import's only sub-expression, so nothing else here
        // needs the generic traversal.
        ExprKind::Import(import) => {
            let headers = match &import.location {
                ImportTarget::Remote(url) => {
                    url.headers.as_ref().map(|headers| {
                        traverse_accumulate(
                            env,
                            &mut NameEnv::new(),
                            nodes,
                            base_location,
                            headers,
                        )
                    })
                }
                _ => None,
            };
            let import_id = cx.push_import(
                base_location.clone(),
                import.map_ref(|_| ()),
                headers,
                expr.span(),
            );
            nodes.push(ImportNode::Import(import_id));
            HirKind::Import(import_id)
        }
        ExprKind::Op(OpKind::BinOp(BinOp::ImportAlt, l, r)) => {
            let mut imports_l = Vec::new();
            let l = traverse_accumulate(
                env,
                name_env,
                &mut imports_l,
                base_location,
                l,
            );
            let mut imports_r = Vec::new();
            let r = traverse_accumulate(
                env,
                name_env,
                &mut imports_r,
                base_location,
                r,
            );
            let alt =
                cx.push_import_alternative(imports_l.into(), imports_r.into());
            nodes.push(ImportNode::Alternative(alt));
            HirKind::ImportAlternative(alt, l, r)
        }
        kind => {
            let kind = kind.map_ref_maybe_binder(|l, e| {
                if let Some(l) = l {
                    name_env.insert_mut(l);
                }
                let hir =
                    traverse_accumulate(env, name_env, nodes, base_location, e);
                if l.is_some() {
                    name_env.remove_mut();
                }
                hir
            });
            HirKind::Expr(kind)
        }
    };
    Hir::new(kind, expr.span())
}

/// Take a list of nodes and recursively resolve them.
fn resolve_nodes<'cx>(
    env: &mut ImportEnv<'cx>,
    nodes: &[ImportNode<'cx>],
) -> Result<(), Error> {
    for &node in nodes {
        match node {
            ImportNode::Import(import) => {
                let res_id = fetch_import(env, import)?;
                env.cx()[import].set_resultid(res_id);
            }
            ImportNode::Alternative(alt) => {
                let alt = &env.cx()[alt];
                match resolve_nodes(env, &alt.left_imports) {
                    Ok(()) => alt.set_selected(true),
                    Err(e) if e.is_recoverable() => {
                        resolve_nodes(env, &alt.right_imports)?;
                        alt.set_selected(false);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(())
}

fn resolve_with_env<'cx>(
    env: &mut ImportEnv<'cx>,
    parsed: Parsed,
) -> Result<Resolved<'cx>, Error> {
    let Parsed(expr, base_location) = parsed;
    let mut nodes = Vec::new();
    // First we collect all imports.
    let resolved = traverse_accumulate(
        env,
        &mut NameEnv::new(),
        &mut nodes,
        &base_location,
        &expr,
    );
    // Then we resolve them and choose sides for the alternatives.
    resolve_nodes(env, &nodes)?;
    Ok(Resolved(resolved))
}

/// Resolves all imports and names. Returns errors if importing failed. Name errors are deferred to
/// typechecking.
pub fn resolve(cx: Ctxt<'_>, parsed: Parsed) -> Result<Resolved<'_>, Error> {
    parsed.resolve_with_env(&mut ImportEnv::new(cx))
}

/// Resolves imports, but refuses to fetch any remote one.
///
/// Local imports read files the caller could have read anyway; a remote import
/// fetches and runs code from a third party. This allows the first without the
/// second, which is what you want for configuration you did not write. Note the
/// check is on where a request would actually go, so a relative import inside
/// an already-remote file is refused too.
pub fn resolve_without_remote_imports(
    cx: Ctxt<'_>,
    parsed: Parsed,
) -> Result<Resolved<'_>, Error> {
    parsed.resolve_with_env(&mut ImportEnv::new(cx).without_remote_imports())
}

/// Resolves names, and errors if we find any imports.
pub fn skip_resolve(
    cx: Ctxt<'_>,
    parsed: Parsed,
) -> Result<Resolved<'_>, Error> {
    let parsed = Parsed::from_expr_without_imports(parsed.0);
    resolve(cx, parsed)
}

impl Parsed {
    fn resolve_with_env<'cx>(
        self,
        env: &mut ImportEnv<'cx>,
    ) -> Result<Resolved<'cx>, Error> {
        resolve_with_env(env, self)
    }
}

pub trait Canonicalize {
    #[must_use]
    fn canonicalize(&self) -> Self;
}

impl Canonicalize for FilePath {
    fn canonicalize(&self) -> FilePath {
        let mut file_path = Vec::new();

        for c in &self.file_path {
            match c.as_ref() {
                // canonicalize(directory₀) = directory₁
                // ───────────────────────────────────────
                // canonicalize(directory₀/.) = directory₁
                "." => {}
                ".." => match file_path.last() {
                    // canonicalize(directory₀) = ε
                    // ────────────────────────────
                    // canonicalize(directory₀/..) = /..
                    None => file_path.push("..".to_string()),

                    // canonicalize(directory₀) = directory₁/..
                    // ──────────────────────────────────────────────
                    // canonicalize(directory₀/..) = directory₁/../..
                    Some(c) if c == ".." => file_path.push("..".to_string()),

                    // canonicalize(directory₀) = directory₁/component
                    // ───────────────────────────────────────────────  ; If "component" is not
                    // canonicalize(directory₀/..) = directory₁         ; ".."
                    Some(_) => {
                        file_path.pop();
                    }
                },

                // canonicalize(directory₀) = directory₁
                // ─────────────────────────────────────────────────────────  ; If no other
                // canonicalize(directory₀/component) = directory₁/component  ; rule matches
                _ => file_path.push(c.clone()),
            }
        }

        FilePath { file_path }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_home(path: impl AsRef<Path>) -> Result<PathBuf, Error> {
    let mut f = PathBuf::new();

    match path.as_ref().strip_prefix("~") {
        Ok(rest) => {
            let home = home::home_dir()
                .ok_or_else(|| Error::from(ImportError::MissingHome))?;
            f.push(home);
            f.push(rest);
        }
        Err(_) => {
            f.push(path);
        }
    }

    Ok(f)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn resolve_home(_path: impl AsRef<Path>) -> Result<PathBuf, Error> {
    panic!("Imports relative to home are not supported on wasm yet");
}

impl<SE: Copy> Canonicalize for ImportTarget<SE> {
    fn canonicalize(&self) -> ImportTarget<SE> {
        match self {
            ImportTarget::Local(prefix, file) => {
                ImportTarget::Local(*prefix, file.canonicalize())
            }
            ImportTarget::Remote(url) => ImportTarget::Remote(URL {
                scheme: url.scheme,
                authority: url.authority.clone(),
                path: url.path.canonicalize(),
                query: url.query.clone(),
                headers: url.headers,
            }),
            ImportTarget::Env(name) => ImportTarget::Env(name.clone()),
            ImportTarget::Missing => ImportTarget::Missing,
        }
    }
}
