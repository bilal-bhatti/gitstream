use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Modified,
    Deleted,
    Untracked,
}

#[derive(Debug, Clone)]
pub enum HunkLine {
    Context(String),
    Added(String),
    Removed(String),
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_range: (u32, u32),
    pub new_range: (u32, u32),
    pub lines: Vec<HunkLine>,
}

#[derive(Debug, Clone)]
pub struct DiffUpdate {
    pub path: PathBuf,
    pub mtime: SystemTime,
    pub status: ChangeKind,
    pub hunks: Vec<Hunk>,
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
}

impl DiffUpdate {
    pub fn is_clean(&self) -> bool {
        self.added == 0
            && self.removed == 0
            && !self.binary
            && !matches!(self.status, ChangeKind::Deleted)
    }
}

#[derive(Default)]
pub struct State {
    by_path: HashMap<PathBuf, DiffUpdate>,
    order: Vec<PathBuf>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, update: DiffUpdate) {
        if update.is_clean() {
            self.drop_path(&update.path);
            return;
        }
        let path = update.path.clone();
        if !self.by_path.contains_key(&path) {
            self.order.push(path.clone());
        }
        self.by_path.insert(path, update);
        self.resort();
    }

    pub fn drop_path(&mut self, path: &Path) {
        if self.by_path.remove(path).is_some() {
            self.order.retain(|p| p.as_path() != path);
        }
    }

    fn resort(&mut self) {
        let by_path = &self.by_path;
        self.order.sort_by(|a, b| {
            by_path[b]
                .mtime
                .cmp(&by_path[a].mtime)
                .then_with(|| a.cmp(b))
        });
    }

    pub fn iter_ordered(&self) -> impl Iterator<Item = &DiffUpdate> {
        self.order.iter().filter_map(|p| self.by_path.get(p))
    }

    pub fn get(&self, path: &Path) -> Option<&DiffUpdate> {
        self.by_path.get(path)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn order(&self) -> &[PathBuf] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn mk(path: &str, mtime: SystemTime, added: u32) -> DiffUpdate {
        DiffUpdate {
            path: PathBuf::from(path),
            mtime,
            status: ChangeKind::Modified,
            hunks: vec![],
            added,
            removed: 0,
            binary: false,
        }
    }

    #[test]
    fn ordering_is_mtime_descending() {
        let mut s = State::new();
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(1);
        let t2 = t0 + Duration::from_secs(2);
        s.apply(mk("a", t1, 1));
        s.apply(mk("b", t2, 1));
        s.apply(mk("c", t0, 1));
        let paths: Vec<_> = s.iter_ordered().map(|u| u.path.clone()).collect();
        assert_eq!(paths, vec![PathBuf::from("b"), PathBuf::from("a"), PathBuf::from("c")]);
    }

    #[test]
    fn clean_update_drops_entry() {
        let mut s = State::new();
        let t = SystemTime::UNIX_EPOCH;
        s.apply(mk("a", t, 1));
        assert_eq!(s.len(), 1);
        s.apply(mk("a", t, 0));
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn replace_keeps_single_entry() {
        let mut s = State::new();
        let t0 = SystemTime::UNIX_EPOCH;
        let t1 = t0 + Duration::from_secs(1);
        s.apply(mk("a", t0, 1));
        s.apply(mk("a", t1, 2));
        assert_eq!(s.len(), 1);
        assert_eq!(s.get(&PathBuf::from("a")).unwrap().added, 2);
    }
}
