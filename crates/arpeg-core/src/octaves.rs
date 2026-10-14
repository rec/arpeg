//! Authored octave layers and their source/layer traversal order.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Traversal {
    #[default]
    NotesFirst,
    OctavesFirst,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Octaves {
    pub offsets: Vec<i64>,
    pub traversal: Traversal,
}

impl Default for Octaves {
    fn default() -> Self {
        Self {
            offsets: vec![0],
            traversal: Traversal::NotesFirst,
        }
    }
}

impl Octaves {
    pub fn indices(&self, size: usize, reverse: bool) -> Vec<(usize, usize)> {
        let mut layers: Vec<_> = (0..self.offsets.len()).collect();
        if reverse {
            layers.reverse();
        }
        match self.traversal {
            Traversal::NotesFirst => layers
                .into_iter()
                .flat_map(|j| (0..size).map(move |i| (i, j)))
                .collect(),
            Traversal::OctavesFirst => (0..size)
                .flat_map(|i| layers.iter().map(move |j| (i, *j)))
                .collect(),
        }
    }
}
