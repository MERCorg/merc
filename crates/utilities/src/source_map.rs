use std::path::Path;

use crate::tagged_index::TagIndex;

/// Tag for [`SourceId`], see [`crate::TagIndex`].
#[derive(Debug)]
pub struct SourceTag;

/// Identifies one file loaded into a [`SourceMap`]. Cheap to copy, only meaningful relative to
/// the `SourceMap` that produced it.
pub type SourceId = TagIndex<usize, SourceTag>;

/// Owns every file loaded into one compilation and assigns each a disjoint slice of one shared,
/// global byte-offset space.
#[derive(Default)]
pub struct SourceMap {
    /// Sorted by `base`, ascending, with no gaps.
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Creates an empty source map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads `path` from disk and registers its contents as a new file.
    pub fn load_file(&mut self, path: &Path) -> std::io::Result<SourceId> {
        let text = std::fs::read_to_string(path)?;
        Ok(self.add(path.display().to_string(), text, false))
    }

    /// Registers `text` directly, under `name`, as ordinary (non-virtual) source — for text with
    /// no on-disk file behind it, such as a single in-memory buffer (LSP editing, an ad hoc
    /// expression string) that still deserves file-accurate rendering.
    pub fn add_text(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        self.add(name.into(), text.into(), false)
    }

    /// Registers `text` as *virtual*: generated content with no real file behind it at all, such
    /// as the system-defined ("Appendix B") built-in declarations. See [`SourceMap::is_virtual`].
    pub fn add_virtual(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        self.add(name.into(), text.into(), true)
    }

    fn add(&mut self, name: String, text: String, is_virtual: bool) -> SourceId {
        let base = self.files.last().map_or(0, |file| file.base + file.text.len());
        let id = TagIndex::new(self.files.len());
        self.files.push(SourceFile {
            name,
            text,
            base,
            is_virtual,
        });
        id
    }

    /// The number of files currently loaded.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Finds which loaded file a global byte offset (as found in a [`crate::Span`]) falls into.
    /// Offsets past the end of every loaded file resolve to the last file, so an out-of-range or
    /// synthetic (e.g. [`crate::Span::default`]) span still renders against something rather than
    /// panicking.
    ///
    /// Panics if no file has been loaded yet.
    pub fn lookup(&self, offset: usize) -> SourceId {
        assert!(!self.files.is_empty(), "SourceMap::lookup on an empty SourceMap");
        match self.files.binary_search_by(|file| file.base.cmp(&offset)) {
            Ok(index) => TagIndex::new(index),
            Err(0) => TagIndex::new(0),
            Err(index) => TagIndex::new((index - 1).min(self.files.len() - 1)),
        }
    }

    /// The full text of the file `id` refers to.
    pub fn text(&self, id: SourceId) -> &str {
        &self.files[id.value()].text
    }

    /// The display name of the file `id` refers to: a real path, or a synthetic name for text
    /// with no file behind it.
    pub fn path(&self, id: SourceId) -> &str {
        &self.files[id.value()].name
    }

    /// Whether `id` was registered via [`SourceMap::add_virtual`] — generated content (e.g. the
    /// system-defined specification) rather than text a user wrote or edited.
    pub fn is_virtual(&self, id: SourceId) -> bool {
        self.files[id.value()].is_virtual
    }

    /// The global offset at which the file `id` refers to starts. A [`crate::Span`] produced
    /// while parsing that file's text alone has `start`/`end` offset by this amount from what
    /// pest reported; subtracting it back off recovers a span local to that file's own text.
    pub(crate) fn base(&self, id: SourceId) -> usize {
        self.files[id.value()].base
    }
}

/// One loaded file: its display name, its text, and the offset at which that text starts within
/// the [`SourceMap`]'s shared, global byte-offset space.
struct SourceFile {
    /// The name shown in rendered diagnostics: a real (relative or absolute) path, or a
    /// synthetic name for text with no file behind it (e.g. `"<builtin>/list.mcrl2"`).
    name: String,
    
    /// The file's full text.
    text: String,

    /// Offset of this file's text within the shared, global byte-offset space: a [`crate::Span`]
    /// produced while parsing this file's text has `start`/`end` values offset by `base` from
    /// what pest reported.
    base: usize,

    /// Whether this file was registered via [`SourceMap::add_virtual`] rather than loaded from
    /// (or standing in for) real, user-authored text.
    is_virtual: bool,
}

#[cfg(test)]
mod tests {
    use super::SourceMap;

    #[test]
    fn test_single_file_lookup_and_accessors() {
        let mut sources = SourceMap::new();
        let id = sources.add_text("spec.mcrl2", "sort D;");
        assert_eq!(sources.file_count(), 1);
        assert_eq!(sources.text(id), "sort D;");
        assert_eq!(sources.path(id), "spec.mcrl2");
        assert!(!sources.is_virtual(id));
        assert_eq!(sources.lookup(0), id);
        assert_eq!(sources.lookup(3), id);
        // Past the end of the only file still resolves to it.
        assert_eq!(sources.lookup(1000), id);
    }

    #[test]
    fn test_multiple_files_get_disjoint_offsets() {
        let mut sources = SourceMap::new();
        let first = sources.add_text("a.mcrl2", "sort D;");
        let second = sources.add_virtual("<builtin>/list.mcrl2", "sort List;");
        assert_eq!(sources.file_count(), 2);
        assert!(!sources.is_virtual(first));
        assert!(sources.is_virtual(second));

        assert_eq!(sources.base(first), 0);
        assert_eq!(sources.base(second), "sort D;".len());

        assert_eq!(sources.lookup(0), first);
        assert_eq!(sources.lookup("sort D;".len() - 1), first);
        assert_eq!(sources.lookup(sources.base(second)), second);
        assert_eq!(sources.lookup(sources.base(second) + 3), second);
    }
}
