use include_dir::{include_dir, Dir, DirEntry};
use itertools::Itertools;

pub(super) static MODULES: Dir = embedded_modules();

const fn embedded_modules() -> Dir<'static> {
    let modules = include_dir!("$CARGO_MANIFEST_DIR/../dbemodules");
    assert!(!modules.entries().is_empty());
    modules
}

pub(super) fn walk_files<'a>(dir: &'a Dir<'a>) -> impl Iterator<Item = &'a DirEntry<'a>> {
    WalkDirIter {
        stack: dir.entries().iter().rev().collect_vec(),
    }
}

struct WalkDirIter<'a> {
    stack: Vec<&'a DirEntry<'a>>,
}

impl<'a> Iterator for WalkDirIter<'a> {
    type Item = &'a DirEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.stack.pop()?;
        self.stack.extend(entry.children().iter().rev());
        Some(entry)
    }
}
