use crate::diagnostic::{Diagnostic, DiagnosticLevel};
use crate::path::{DiagnosticPath, DiagnosticPathSegment};
use smallvec::SmallVec;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Display;

#[derive(Debug)]
pub struct DiagnosticContext {
    pub diagnostics: BTreeMap<String, BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>>>,
    path: DiagnosticPath,
}

impl Default for DiagnosticContext {
    fn default() -> Self {
        DiagnosticContext {
            diagnostics: Default::default(),
            path: DiagnosticPath::empty(),
        }
    }
}

impl DiagnosticContext {
    pub fn enter(&mut self, ident: impl Display) -> DiagnosticContextMut<'_> {
        let entry = self.diagnostics.entry(ident.to_string()).or_default();
        DiagnosticContextMut {
            diagnostics: entry,
            path: &mut self.path,
            pop_on_exit: false,
        }
    }

    pub fn enter_readonly(&mut self, ident: impl Display) -> DiagnosticContextRef<'_> {
        let entry = self.diagnostics.entry(ident.to_string()).or_default();
        DiagnosticContextRef {
            diagnostics: entry,
            path: &mut self.path,
            pop_on_exit: false,
        }
    }

    pub fn enter_new(&mut self, ident: impl Display) -> DiagnosticContextMut<'_> {
        if self.diagnostics.contains_key(&ident.to_string()) {
            panic!("Diagnostic context already exists for {}", ident);
        }

        self.enter(ident)
    }
}

impl<'a> DiagnosticContextMut<'a> {
    pub fn emit(&mut self, info: miette::Report, level: DiagnosticLevel) {
        self.diagnostics
            .entry(self.path.clone())
            .or_default()
            .push(Diagnostic { info, level });
    }

    pub fn emit_error(&mut self, info: miette::Report) {
        self.emit(info, DiagnosticLevel::Error);
    }

    pub fn emit_warning(&mut self, info: miette::Report) {
        self.emit(info, DiagnosticLevel::Warning);
    }

    /// Clears all warnings originating from the current context or its children.
    pub fn clear_downstream(&mut self) {
        self.diagnostics
            .retain(|path, _| !path.starts_with(self.path));
    }

    /// Returns a read-only view of this context
    pub fn as_readonly(&mut self) -> DiagnosticContextRef<'_> {
        DiagnosticContextRef {
            diagnostics: self.diagnostics,
            path: &mut self.path,
            pop_on_exit: false,
        }
    }
}

pub type DiagnosticContextRef<'a> =
    DiagnosticContextRefHolder<'a, &'a BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>>>;
pub type DiagnosticContextMut<'a> =
    DiagnosticContextRefHolder<'a, &'a mut BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>>>;

pub struct DiagnosticContextRefHolder<'a, T: 'a + ContextLike> {
    diagnostics: T,
    path: &'a mut DiagnosticPath,
    pop_on_exit: bool,
}

impl<'a, T: 'a + ContextLike> DiagnosticContextRefHolder<'a, T>
where
    for<'b> T::Target<'b>: ContextLike,
{
    pub fn enter(
        &mut self,
        segment: impl Into<DiagnosticPathSegment>,
    ) -> DiagnosticContextRefHolder<'_, T::Target<'_>> {
        self.path.push(segment);
        DiagnosticContextRefHolder {
            diagnostics: self.diagnostics.make_ref(),
            path: self.path,
            pop_on_exit: true,
        }
    }

    pub fn enter_index(&mut self, index: usize) -> DiagnosticContextRefHolder<'_, T::Target<'_>> {
        self.enter(DiagnosticPathSegment::Index(index))
    }

    pub fn enter_field(
        &mut self,
        field: impl Into<Cow<'static, str>>,
    ) -> DiagnosticContextRefHolder<'_, T::Target<'_>> {
        self.enter(DiagnosticPathSegment::Field(field.into()))
    }

    pub fn enter_variant(
        &mut self,
        variant: impl Into<Cow<'static, str>>,
    ) -> DiagnosticContextRefHolder<'_, T::Target<'_>> {
        self.enter(DiagnosticPathSegment::Variant(variant.into()))
    }

    pub fn enter_inline(&mut self) -> DiagnosticContextRefHolder<'_, T::Target<'_>> {
        DiagnosticContextRefHolder {
            diagnostics: self.diagnostics.make_ref(),
            path: self.path,
            pop_on_exit: false,
        }
    }

    pub fn path(&self) -> &DiagnosticPath {
        self.path
    }

    /// Returns reports of the current context only.
    pub fn get_reports_shallow(&self) -> impl Iterator<Item = &Diagnostic> {
        let p = self.path();
        self.diagnostics
            .as_btreemap()
            .get(p)
            .into_iter()
            .flat_map(|v| v.iter())
    }

    /// Returns reports of the current context and all its children.
    pub fn get_reports_deep(
        &self,
    ) -> impl Iterator<Item = (&DiagnosticPath, impl IntoIterator<Item = &Diagnostic>)> {
        let p = self.path();
        self.diagnostics
            .as_btreemap()
            .range(p..)
            .take_while(|i| i.0.starts_with(p))
    }
}

impl<'a, T: 'a + ContextLike> Drop for DiagnosticContextRefHolder<'a, T> {
    fn drop(&mut self) {
        if self.pop_on_exit {
            self.path.pop();
        }
    }
}

pub trait ContextLike: sealed::Sealed {
    fn make_ref(&mut self) -> Self::Target<'_>;
    fn as_btreemap(&self) -> &BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>>;
}

impl<'a> ContextLike for &'a BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>> {
    fn make_ref(&mut self) -> Self::Target<'_> {
        self
    }

    fn as_btreemap(&self) -> &BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>> {
        self
    }
}

impl<'a> ContextLike for &'a mut BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>> {
    fn make_ref(&mut self) -> Self::Target<'_> {
        self
    }

    fn as_btreemap(&self) -> &BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>> {
        self
    }
}

mod sealed {
    use crate::diagnostic::Diagnostic;
    use crate::path::DiagnosticPath;
    use smallvec::SmallVec;
    use std::collections::BTreeMap;

    pub trait Sealed {
        type Target<'b>;
    }

    impl<'a> Sealed for &'a BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>> {
        type Target<'b> = &'b BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>>;
    }

    impl<'a> Sealed for &'a mut BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>> {
        type Target<'b> = &'b mut BTreeMap<DiagnosticPath, SmallVec<[Diagnostic; 1]>>;
    }
}
