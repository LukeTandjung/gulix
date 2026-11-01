//! Pure preview rules. No Typst, Tern, filesystem, clock, or task APIs belong here.

use std::ops::Range;

use vstd::prelude::*;

verus! {

/// Stable identity assigned by app-layer reconciliation across reparses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FragmentId(pub u64);

/// Only supported block constructs enter the preview pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentKind {
    InlineMath,
    DisplayMath,
    CodeBlock,
    Table,
    Image,
    Figure,
}

/// One flat display segment and its mapping back to authoritative source.
/// Discovery must partition overlapping syntax into non-overlapping segments rather
/// than emitting both a paragraph and its inline children as separate previews.
#[derive(Debug, PartialEq, Eq)]
pub struct Fragment {
    pub id: FragmentId,
    /// Half-open character offsets into the authoritative Helix buffer.
    pub range: Range<usize>,
    pub kind: FragmentKind,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FragmentState {
    pub fragment: Fragment,
    pub rendered: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PreviewState {
    pub fragments: Vec<FragmentState>,
    pub active: Option<FragmentId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidFragments {
    UnorderedOrOverlapping,
    DuplicateId,
}

} // verus!

verus! {

impl PreviewState {
    /// Initial state has no active fragment and no current render.
    pub fn new(fragments: Vec<Fragment>) -> Result<Self, InvalidFragments> {
        validate_fragments(&fragments)?;

        Ok(Self {
            fragments: fragments
                .into_iter()
                .map(|fragment| FragmentState {
                    fragment,
                    rendered: false,
                })
                .collect(),
            active: None,
        })
    }

    /// Read-only input for app-layer reconciliation after a source revision.
    pub fn fragments(&self) -> impl Iterator<Item = &Fragment> {
        self.fragments.iter().map(|state| &state.fragment)
    }

    /// Context changes hide old renders without discarding their cached artifacts.
    pub fn invalidate_renders(&mut self)
        ensures
            final(self).fragments.len() == old(self).fragments.len(),
            forall|index: int|
                0 <= index < final(self).fragments.len()
                    ==> !final(self).fragments[index].rendered,
            final(self).active == old(self).active,
    {
        let mut index = 0;
        while index < self.fragments.len()
            invariant
                index <= self.fragments.len(),
                self.fragments.len() == old(self).fragments.len(),
                forall|previous: int|
                    0 <= previous < index ==> !self.fragments[previous].rendered,
                self.active == old(self).active,
            decreases self.fragments.len() - index,
        {
            self.fragments[index].rendered = false;
            index += 1;
        }
    }

    pub open spec fn has_id(&self, id: FragmentId) -> bool {
        exists|index: int| 0 <= index < self.fragments.len()
            && self.fragments[index].fragment.id == id
    }

    pub open spec fn has_rendered(&self, id: FragmentId) -> bool {
        exists|index: int|
            0 <= index < self.fragments.len()
                && self.fragments[index].fragment.id == id
                && self.fragments[index].rendered
                && forall|previous: int|
                    0 <= previous < index
                        ==> self.fragments[previous].fragment.id != id
    }

    proof fn has_rendered_at(&self, index: int, id: FragmentId)
        requires
            0 <= index < self.fragments.len(),
            self.fragments[index].fragment.id == id,
            forall|previous: int|
                0 <= previous < index ==> self.fragments[previous].fragment.id != id,
        ensures
            self.has_rendered(id) == self.fragments[index].rendered,
    {
        reveal(PreviewState::has_rendered);

        if self.fragments[index].rendered {
            assert(self.has_rendered(id));
        } else {
            assert forall|other: int|
                0 <= other < self.fragments.len()
                    && self.fragments[other].fragment.id == id
                    && self.fragments[other].rendered
                    && (forall|previous: int|
                        0 <= previous < other
                            ==> self.fragments[previous].fragment.id != id)
                implies false by {
                if other < index {
                    assert(self.fragments[other].fragment.id != id);
                } else if other == index {
                    assert(!self.fragments[other].rendered);
                } else {
                    assert(self.fragments[index].fragment.id != id);
                }
            }

            assert(!self.has_rendered(id));
        }
    }

    fn find_index(&self, id: FragmentId) -> (result: Option<usize>)
        ensures
            result is Some ==> 0 <= result->Some_0 < self.fragments.len()
                && self.fragments[result->Some_0 as int].fragment.id.0 == id.0,
            result is Some ==> self.has_id(id),
            result is None ==> !self.has_id(id),
            result is Some ==> forall|other: int| 0 <= other < result->Some_0
                ==> self.fragments[other].fragment.id.0 != id.0,
    {
        proof {
            reveal(PreviewState::has_id);
        }

        self.find_index_from(id, 0)
    }

    fn find_index_from(&self, id: FragmentId, start: usize) -> (result: Option<usize>)
        requires
            start <= self.fragments.len(),
        ensures
            result is Some ==> start <= result->Some_0 < self.fragments.len()
                && self.fragments[result->Some_0 as int].fragment.id.0 == id.0,
            result is Some ==> forall|other: int| start <= other < result->Some_0
                ==> self.fragments[other].fragment.id.0 != id.0,
            result is None ==> forall|other: int| start <= other < self.fragments.len()
                ==> self.fragments[other].fragment.id.0 != id.0,
        decreases self.fragments.len() - start,
    {
        if start == self.fragments.len() {
            None
        } else if self.fragments[start].fragment.id.0 == id.0 {
            Some(start)
        } else {
            self.find_index_from(id, start + 1)
        }
    }

    fn contains(&self, id: FragmentId) -> (result: bool)
        ensures
            result == self.has_id(id),
    {
        self.find_index(id).is_some()
    }

    /// Clicking a fragment makes it the sole active, raw fragment.
    pub fn activate(&mut self, id: FragmentId) -> (result: bool)
        ensures
            result ==> final(self).active == Some(id),
            !result ==> final(self).active == old(self).active,
            result == old(self).has_id(id),
            result ==> final(self).has_id(id),
            final(self).fragments@ == old(self).fragments@,
    {
        proof {
            reveal(PreviewState::has_id);
        }

        if self.contains(id) {
            self.active = Some(id);

            assert(self.fragments@ == old(self).fragments@);
            assert(final(self).has_id(id) == self.has_id(id));

            true
        } else {
            false
        }
    }

    pub fn deactivate(&mut self)
        ensures
            final(self).active is None,
            final(self).fragments@ == old(self).fragments@,
    {
        self.active = None;
    }

    /// Source replacement preserves active identity. The app restores only
    /// fragments whose complete request still matches a retained artifact;
    /// every other fragment starts without a current render.
    pub fn replace_fragments(&mut self, fragments: Vec<Fragment>) -> (result: Result<(), InvalidFragments>)
        ensures
            fragments.len() <= 1 ==> result is Ok,
            result is Err ==> *final(self) == *old(self),
    {
        validate_fragments(&fragments)?;

        self.fragments = fragments
            .into_iter()
            .map(|fragment| FragmentState {
                fragment,
                rendered: false,
            })
            .collect();

        if let Some(id) = self.active {
            if !self.contains(id) {
                self.active = None;
            }
        }

        Ok(())
    }

    /// Only inactive fragments without a current artifact need asynchronous work.
    pub fn needs_render(&self, id: FragmentId) -> (result: bool)
        ensures
            result == (self.active != Some(id) && self.has_id(id) && !self.has_rendered(id)),
    {
        proof {
            reveal(PreviewState::has_id);
            reveal(PreviewState::has_rendered);
        }

        if let Some(active) = self.active {
            if active.0 == id.0 {
                return false;
            }
        }

        match self.find_index(id) {
            Some(index) => {
                assert(self.has_id(id));

                proof {
                    self.has_rendered_at(index as int, id);
                }

                !self.fragments[index].rendered
            },
            None => false,
        }
    }

    /// Only inactive fragments with an accepted artifact render.
    pub fn is_rendered(&self, id: FragmentId) -> (result: Option<bool>)
        ensures
            (result is None) == !self.has_id(id),
            result is Some
                ==> result->Some_0 == (self.active != Some(id) && self.has_rendered(id)),
    {
        let Some(index) = self.find_index(id) else {
            return None;
        };

        proof {
            reveal(PreviewState::has_id);
            reveal(PreviewState::has_rendered);
        }

        assert(self.has_id(id));

        proof {
            self.has_rendered_at(index as int, id);
        }

        let inactive = match self.active {
            Some(active) => active.0 != id.0,
            None => true,
        };

        Some(inactive && self.fragments[index].rendered)
    }

    /// The app accepts only jobs whose complete request still matches.
    pub fn record_render(&mut self, id: FragmentId) -> (result: bool)
        ensures
            !old(self).has_id(id) ==> !result && *final(self) == *old(self),
            old(self).has_id(id)
                ==> result
                    && final(self).has_rendered(id)
                    && final(self).active == old(self).active
                    && final(self).has_id(id),
    {
        let Some(index) = self.find_index(id) else {
            return false;
        };

        proof {
            reveal(PreviewState::has_id);
        }

        self.fragments[index].rendered = true;

        proof {
            reveal(PreviewState::has_id);
            reveal(PreviewState::has_rendered);
            assert(self.fragments@[index as int].fragment.id == id);
            assert(self.has_id(id));
            self.has_rendered_at(index as int, id);
            assert(self.has_rendered(id));
        }

        assert(self.active == old(self).active);

        true
    }
}

fn validate_fragments(fragments: &[Fragment]) -> (result: Result<(), InvalidFragments>)
    ensures
        fragments.len() <= 1 ==> result is Ok,
{
    let mut index = 0;
    while index < fragments.len()
        invariant
            index <= fragments.len(),
        decreases fragments.len() - index
    {
        let mut previous = 0;
        while previous < index
            invariant
                previous <= index,
                index < fragments.len(),
            decreases index - previous
        {
            if fragments[previous].id == fragments[index].id {
                return Err(InvalidFragments::DuplicateId);
            }
            previous += 1;
        }

        if index > 0 && fragments[index - 1].range.end > fragments[index].range.start {
            return Err(InvalidFragments::UnorderedOrOverlapping);
        }
        index += 1;
    }

    Ok(())
}

} // verus!
