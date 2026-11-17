//! The layers of the GIS viewer, as a tree like the layer list of QGIS: map layers (georeferenced images) in groups that
//! can be nested, hidden and moved around, plus two built-in layers that only have a switch: the basemap (satellite
//! imagery or the street map) and the specimen symbols.
//!
//! The built-in layers are pinned: they come first in the list, cannot be moved or removed, are drawn at fixed places
//! (the basemap below everything, the symbols above everything) and the sidebar shows them above a separator. The
//! added layers follow, in list order, top to bottom, with every group followed by its children, so a group and
//! everything in it is one contiguous stretch of the list. What is higher in the list is drawn above what is lower.
//! Everything here is plain list handling, tested without a window; the pixels and the textures live in `gis_ui.rs`.

use serde_json::{json, Value};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Group,
    /// The basemap tiles (cannot be removed, only hidden).
    Basemap,
    /// The strike / dip symbols of the specimens (cannot be removed, only hidden).
    Specimens,
    /// A georeferenced image.
    Map,
    /// Points, lines and polygons from a shapefile, GeoPackage or GeoJSON.
    Vector,
}

impl Kind {
    /// The basemap and the symbols: pinned, no moving, no removing.
    pub fn is_builtin(self) -> bool {
        matches!(self, Kind::Basemap | Kind::Specimens)
    }

    fn token(self) -> &'static str {
        match self {
            Kind::Group => "group",
            Kind::Basemap => "basemap",
            Kind::Specimens => "specimens",
            Kind::Map => "map",
            Kind::Vector => "vector",
        }
    }

    fn from_token(t: &str) -> Option<Kind> {
        Some(match t {
            "group" => Kind::Group,
            "basemap" => Kind::Basemap,
            "specimens" => Kind::Specimens,
            "map" => Kind::Map,
            "vector" => Kind::Vector,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u32,
    pub kind: Kind,
    pub name: String,
    pub visible: bool,
    /// A group shows its children (unfolded).
    pub open: bool,
    pub parent: Option<u32>,
}

/// Where a dragged item lands relative to the row it is dropped on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drop {
    Before,
    After,
    /// Into a group, as its first child.
    Inside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerTree {
    pub items: Vec<Item>,
    next_id: u32,
}

impl Default for LayerTree {
    /// The symbols above the basemap.
    fn default() -> Self {
        LayerTree {
            items: vec![
                Item { id: 1, kind: Kind::Specimens, name: "Specimen symbols".into(), visible: true, open: true, parent: None },
                Item { id: 2, kind: Kind::Basemap, name: "Backdrop".into(), visible: false, open: true, parent: None },
            ],
            next_id: 3,
        }
    }
}

impl LayerTree {
    pub fn get(&self, id: u32) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.items.iter().position(|i| i.id == id)
    }

    pub fn first_of_kind(&self, k: Kind) -> Option<u32> {
        self.items.iter().find(|i| i.kind == k).map(|i| i.id)
    }

    fn depth(&self, id: u32) -> usize {
        let mut d = 0;
        let mut cur = self.get(id).and_then(|i| i.parent);
        while let Some(p) = cur {
            d += 1;
            cur = self.get(p).and_then(|i| i.parent);
            if d > 64 {
                break;
            }
        }
        d
    }

    /// The item at `idx` and everything below it in the tree (a contiguous stretch of the list).
    pub fn subtree(&self, idx: usize) -> Range<usize> {
        let d = self.depth(self.items[idx].id);
        let end = (idx + 1..self.items.len()).find(|&j| self.depth(self.items[j].id) <= d).unwrap_or(self.items.len());
        idx..end
    }

    pub fn is_inside(&self, id: u32, group: u32) -> bool {
        let mut cur = self.get(id).and_then(|i| i.parent);
        while let Some(p) = cur {
            if p == group {
                return true;
            }
            cur = self.get(p).and_then(|i| i.parent);
        }
        false
    }

    /// An item is shown when it and all the groups around it are switched on.
    pub fn effective_visible(&self, id: u32) -> bool {
        let Some(mut it) = self.get(id) else { return false };
        loop {
            if !it.visible {
                return false;
            }
            match it.parent.and_then(|p| self.get(p)) {
                Some(p) => it = p,
                None => return true,
            }
        }
    }

    /// The built-in layers (symbols, then basemap) as indices into the list.
    pub fn builtin_rows(&self) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, i)| i.kind.is_builtin()).map(|(k, _)| k).collect()
    }

    /// The rows of the list of added layers as they are shown: every item with its depth, the children of folded groups
    /// left out. The built-in layers are not among them (see `builtin_rows`).
    pub fn rows(&self) -> Vec<(usize, usize)> {
        let mut out = vec![];
        let mut skip_until = 0;
        for (i, it) in self.items.iter().enumerate() {
            if i < skip_until || it.kind.is_builtin() {
                continue;
            }
            out.push((i, self.depth(it.id)));
            if it.kind == Kind::Group && !it.open {
                skip_until = self.subtree(i).end;
            }
        }
        out
    }

    /// What is drawn, from the bottom to the top: the basemap, then the shown added layers (the lowest of the list first),
    /// then the specimen symbols.
    pub fn draw_order(&self) -> Vec<(u32, Kind)> {
        let pick = |k: Kind| self.items.iter().find(|i| i.kind == k && i.visible).map(|i| (i.id, i.kind));
        let mut out: Vec<(u32, Kind)> = pick(Kind::Basemap).into_iter().collect();
        out.extend(self.items.iter().rev().filter(|i| !i.kind.is_builtin() && i.kind != Kind::Group && self.effective_visible(i.id)).map(|i| (i.id, i.kind)));
        out.extend(pick(Kind::Specimens));
        out
    }

    fn fresh_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Where a new layer or group goes: at the top of the added layers, right behind the built-in ones (it is above the
    /// older maps and the basemap, and the symbols are drawn over it anyway).
    fn insert_point(&self) -> usize {
        self.items.iter().take_while(|i| i.kind.is_builtin()).count()
    }

    /// A new map layer; returns its id.
    pub fn add_map(&mut self, name: &str) -> u32 {
        let id = self.fresh_id();
        let at = self.insert_point();
        self.items.insert(at, Item { id, kind: Kind::Map, name: name.to_string(), visible: true, open: true, parent: None });
        id
    }

    /// A new vector layer (shapefile, GeoPackage table, GeoJSON); returns its id.
    pub fn add_vector(&mut self, name: &str) -> u32 {
        let id = self.fresh_id();
        let at = self.insert_point();
        self.items.insert(at, Item { id, kind: Kind::Vector, name: name.to_string(), visible: true, open: true, parent: None });
        id
    }

    /// Adds a whole block of layers and groups at the top of the added layers, keeping their order: each row is
    /// `(kind, name, visible, open, parent)` where `parent` is the position of an earlier row of the block (a group).
    /// Returns the ids in the same order.
    pub fn insert_block(&mut self, rows: &[(Kind, String, bool, bool, Option<usize>)]) -> Vec<u32> {
        let at = self.insert_point();
        let mut ids: Vec<u32> = vec![];
        for (k, (kind, name, visible, open, parent)) in rows.iter().enumerate() {
            let id = self.fresh_id();
            let parent = parent.filter(|p| *p < k).map(|p| ids[p]);
            self.items.insert(at + k, Item { id, kind: *kind, name: name.clone(), visible: *visible, open: *open, parent });
            ids.push(id);
        }
        ids
    }

    /// A new, empty group.
    pub fn add_group(&mut self, name: &str) -> u32 {
        let id = self.fresh_id();
        let at = self.insert_point();
        self.items.insert(at, Item { id, kind: Kind::Group, name: name.to_string(), visible: true, open: true, parent: None });
        id
    }

    /// Removes a map layer or a group (the layers of a group move up into the group's place). The basemap and the
    /// symbols cannot be removed. Returns the ids of the map layers that went with it.
    pub fn remove(&mut self, id: u32) -> Vec<u32> {
        let Some(idx) = self.index_of(id) else { return vec![] };
        match self.items[idx].kind {
            Kind::Basemap | Kind::Specimens => vec![],
            Kind::Map | Kind::Vector => {
                self.items.remove(idx);
                vec![id]
            }
            Kind::Group => {
                let parent = self.items[idx].parent;
                for it in self.items.iter_mut().filter(|i| i.parent == Some(id)) {
                    it.parent = parent;
                }
                self.items.remove(idx);
                vec![]
            }
        }
    }

    /// Removes a group together with everything in it (or a map layer). Returns the ids of the map layers that went.
    pub fn remove_subtree(&mut self, id: u32) -> Vec<u32> {
        let Some(idx) = self.index_of(id) else { return vec![] };
        if self.items[idx].kind.is_builtin() {
            return vec![];
        }
        let range = self.subtree(idx);
        self.items.drain(range).filter(|i| matches!(i.kind, Kind::Map | Kind::Vector)).map(|i| i.id).collect()
    }

    /// Moves an item (with everything in it) next to or into the target row. Refused (false) for a drop onto itself or
    /// into its own contents, and for `Inside` on something that is not a group.
    pub fn move_item(&mut self, id: u32, target: u32, drop: Drop) -> bool {
        let (Some(from), Some(to)) = (self.index_of(id), self.index_of(target)) else { return false };
        if id == target || self.is_inside(target, id) || self.items[from].kind.is_builtin() || self.items[to].kind.is_builtin() {
            return false;
        }
        if drop == Drop::Inside && self.items[to].kind != Kind::Group {
            return false;
        }
        let range = self.subtree(from);
        let moved: Vec<Item> = self.items.drain(range).collect();
        let to = self.index_of(target).unwrap_or(0);
        // after an open group means as its first child
        let drop = if drop == Drop::After && self.items[to].kind == Kind::Group && self.items[to].open { Drop::Inside } else { drop };
        let (at, parent) = match drop {
            Drop::Before => (to, self.items[to].parent),
            Drop::After => (self.subtree(to).end, self.items[to].parent),
            Drop::Inside => {
                self.items[to].open = true;
                (to + 1, Some(target))
            }
        };
        let mut moved = moved;
        moved[0].parent = parent;
        for (k, m) in moved.into_iter().enumerate() {
            self.items.insert(at + k, m);
        }
        true
    }

    /// Switches a group or a layer; a group takes its layers along (they keep their own switch).
    pub fn set_visible(&mut self, id: u32, v: bool) {
        if let Some(i) = self.get_mut(id) {
            i.visible = v;
        }
    }

    /// The tree as JSON values for the preferences file.
    pub fn to_values(&self) -> Vec<Value> {
        self.items.iter().map(|i| json!({"id": i.id, "kind": i.kind.token(), "name": i.name, "visible": i.visible, "open": i.open, "parent": i.parent})).collect()
    }

    /// The tree from the preferences; `None` for anything that is not a sound tree (the default one is used then).
    /// The basemap and the symbols are always there.
    pub fn from_values(v: &[Value]) -> Option<LayerTree> {
        let mut items: Vec<Item> = vec![];
        for x in v {
            let id = u32::try_from(x.get("id")?.as_u64()?).ok()?;
            let kind = Kind::from_token(x.get("kind")?.as_str()?)?;
            let parent = match x.get("parent") {
                Some(Value::Null) | None => None,
                Some(p) => Some(u32::try_from(p.as_u64()?).ok()?),
            };
            if items.iter().any(|i| i.id == id) {
                return None;
            }
            // a parent has to be a group that comes before its child
            if let Some(p) = parent {
                if !items.iter().any(|i| i.id == p && i.kind == Kind::Group) {
                    return None;
                }
            }
            items.push(Item {
                id,
                kind,
                name: x.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                visible: x.get("visible").and_then(|b| b.as_bool()).unwrap_or(true),
                open: x.get("open").and_then(|b| b.as_bool()).unwrap_or(true),
                parent,
            });
        }
        let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        // the built-in layers are pinned at the front and belong to no group
        let (mut builtin, rest): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|i| i.kind.is_builtin());
        builtin.sort_by_key(|i| i.kind != Kind::Specimens);
        for b in &mut builtin {
            b.parent = None;
        }
        let items: Vec<Item> = builtin.into_iter().chain(rest).collect();
        // a layer inside a group that is a built-in one cannot be: groups are all in `rest`, parents stay valid
        let mut t = LayerTree { items, next_id };
        // children must follow their parent without a gap: the stretch of a group has to hold exactly its descendants
        for i in 0..t.items.len() {
            if t.items[i].kind == Kind::Group {
                let r = t.subtree(i);
                if (r.start + 1..r.end).any(|j| !t.is_inside(t.items[j].id, t.items[i].id)) || t.items.iter().enumerate().any(|(j, it)| it.parent == Some(t.items[i].id) && !(r.start < j && j < r.end)) {
                    return None;
                }
            }
        }
        for kind in [Kind::Specimens, Kind::Basemap] {
            if t.first_of_kind(kind).is_none() {
                let id = t.fresh_id();
                let (name, visible) = if kind == Kind::Specimens { ("Specimen symbols", true) } else { ("Backdrop", false) };
                let at = if kind == Kind::Specimens { 0 } else { t.items.iter().take_while(|i| i.kind.is_builtin()).count() };
                t.items.insert(at, Item { id, kind, name: name.into(), visible, open: true, parent: None });
            }
        }
        // the basemap of older preferences was called after its source; it is the Backdrop now
        for it in t.items.iter_mut().filter(|i| i.kind == Kind::Basemap) {
            it.name = "Backdrop".into();
        }
        // the places of the two built-in layers cannot be inside a group that is missing; a second copy is dropped
        for kind in [Kind::Specimens, Kind::Basemap] {
            let first = t.first_of_kind(kind);
            t.items.retain(|i| i.kind != kind || Some(i.id) == first);
        }
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(t: &LayerTree) -> Vec<String> {
        t.items.iter().map(|i| i.name.clone()).collect()
    }

    /// Symbols and basemap (pinned), then group G holding B and A.
    fn tree() -> (LayerTree, u32, u32, u32) {
        let mut t = LayerTree::default();
        let a = t.add_map("A");
        let b = t.add_map("B");
        let g = t.add_group("G");
        assert!(t.move_item(b, g, Drop::Inside));
        assert!(t.move_item(a, b, Drop::After));
        (t, g, a, b)
    }

    #[test]
    fn a_block_of_layers_goes_in_at_the_top_in_its_own_order_with_its_groups() {
        let mut t = LayerTree::default();
        let old = t.add_map("Old");
        let rows = vec![
            (Kind::Group, "G".to_string(), true, false, None),
            (Kind::Vector, "a".to_string(), false, true, Some(0)),
            (Kind::Map, "b".to_string(), true, true, Some(0)),
            (Kind::Vector, "c".to_string(), true, true, None),
        ];
        let ids = t.insert_block(&rows);
        assert_eq!(names(&t), vec!["Specimen symbols", "Backdrop", "G", "a", "b", "c", "Old"]);
        assert_eq!((t.get(ids[1]).unwrap().parent, t.get(ids[3]).unwrap().parent), (Some(ids[0]), None));
        assert_eq!((t.get(ids[0]).unwrap().open, t.get(ids[1]).unwrap().visible), (false, false));
        assert!(t.get(old).is_some());
        // a parent that is not an earlier row is ignored
        let ids = t.insert_block(&[(Kind::Vector, "x".to_string(), true, true, Some(5))]);
        assert_eq!(t.get(ids[0]).unwrap().parent, None);
    }

    #[test]
    fn vector_layers_are_added_removed_and_saved_like_maps() {
        let mut t = LayerTree::default();
        let m = t.add_map("Geology");
        let v = t.add_vector("Faults");
        // the newest added layer is the first one below the built-in ones
        assert_eq!(t.items.iter().map(|i| i.kind).collect::<Vec<_>>(), vec![Kind::Specimens, Kind::Basemap, Kind::Vector, Kind::Map]);
        let g = t.add_group("G");
        assert!(t.move_item(v, g, Drop::Inside));
        // it is drawn between the basemap and the symbols, like a map
        assert!(t.draw_order().iter().any(|(id, k)| *id == v && *k == Kind::Vector));
        // saved and read back
        let back = LayerTree::from_values(&t.to_values()).unwrap();
        assert_eq!(back.get(v).map(|i| (i.kind, i.parent)), Some((Kind::Vector, Some(g))));
        // removing the group with its contents reports the vector layer too; a map alone reports itself
        assert_eq!(t.remove_subtree(g), vec![v]);
        assert_eq!(t.remove(m), vec![m]);
    }

    #[test]
    fn the_built_in_layers_come_first_and_new_layers_go_to_the_top_of_the_added_ones() {
        let mut t = LayerTree::default();
        assert_eq!(names(&t), ["Specimen symbols", "Backdrop"]);
        t.add_map("A");
        t.add_map("B");
        assert_eq!(names(&t), ["Specimen symbols", "Backdrop", "B", "A"]);
        assert_eq!(t.builtin_rows(), vec![0, 1]);
        assert_eq!(t.rows().iter().map(|r| r.0).collect::<Vec<_>>(), vec![2, 3]);
    }

    #[test]
    fn the_basemap_is_drawn_first_the_symbols_last_whatever_the_list_says() {
        let mut t = LayerTree::default();
        let a = t.add_map("A");
        let b = t.add_map("B");
        let base = t.first_of_kind(Kind::Basemap).unwrap();
        let sp = t.first_of_kind(Kind::Specimens).unwrap();
        // the basemap is off until switched on
        assert!(!t.effective_visible(base));
        assert_eq!(t.draw_order().iter().map(|d| d.0).collect::<Vec<_>>(), vec![a, b, sp]);
        t.set_visible(base, true);
        assert_eq!(t.draw_order().iter().map(|d| d.0).collect::<Vec<_>>(), vec![base, a, b, sp]);
        // the symbols can be switched off
        t.set_visible(sp, false);
        assert_eq!(t.draw_order().iter().map(|d| d.0).collect::<Vec<_>>(), vec![base, a, b]);
        // moving the maps around never puts them over the symbols or under the basemap
        assert!(t.move_item(b, a, Drop::After));
        assert_eq!(t.draw_order().iter().map(|d| d.0).collect::<Vec<_>>(), vec![base, b, a]);
    }

    #[test]
    fn the_built_in_layers_cannot_be_moved_targeted_or_removed() {
        let (mut t, g, a, _) = tree();
        let base = t.first_of_kind(Kind::Basemap).unwrap();
        let sp = t.first_of_kind(Kind::Specimens).unwrap();
        assert!(!t.move_item(a, base, Drop::Before) && !t.move_item(a, sp, Drop::After) && !t.move_item(g, base, Drop::After));
        assert!(!t.move_item(base, a, Drop::Before) && !t.move_item(sp, g, Drop::Inside));
        assert!(t.remove(base).is_empty() && t.remove(sp).is_empty() && t.remove_subtree(base).is_empty());
        assert!(t.get(base).is_some() && t.get(sp).is_some());
        assert_eq!(names(&t)[..2], ["Specimen symbols", "Backdrop"]);
    }

    #[test]
    fn groups_nest_and_their_switch_hides_everything_in_them() {
        let (mut t, g, a, b) = tree();
        assert_eq!(names(&t), ["Specimen symbols", "Backdrop", "G", "B", "A"]);
        assert_eq!((t.get(a).unwrap().parent, t.get(b).unwrap().parent), (Some(g), Some(g)));
        assert_eq!(t.subtree(2), 2..5);
        assert!(t.effective_visible(a));
        t.set_visible(g, false);
        assert!(!t.effective_visible(a) && !t.effective_visible(b));
        // the layers keep their own switch
        assert!(t.get(a).unwrap().visible);
        assert!(t.draw_order().iter().all(|(id, _)| *id != a && *id != b));
        t.set_visible(g, true);
        t.set_visible(a, false);
        assert!(t.effective_visible(b) && !t.effective_visible(a));
    }

    #[test]
    fn folded_groups_hide_their_rows() {
        let (mut t, g, ..) = tree();
        assert_eq!(t.rows().iter().map(|r| r.1).collect::<Vec<_>>(), vec![0, 1, 1]);
        t.get_mut(g).unwrap().open = false;
        assert_eq!(t.rows().iter().map(|r| r.0).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn moving_before_after_and_inside() {
        let (mut t, g, a, b) = tree();
        // B out of the group: before the group, then after it
        assert!(t.move_item(b, g, Drop::Before));
        assert_eq!(names(&t)[2..], ["B", "G", "A"]);
        assert_eq!(t.get(b).unwrap().parent, None);
        assert!(t.move_item(b, a, Drop::After));
        assert_eq!(names(&t)[2..], ["G", "A", "B"]);
        assert_eq!((t.get(a).unwrap().parent, t.get(b).unwrap().parent), (Some(g), Some(g)));
        // A out of the group, below the group (After a group that is open means into it, so close it first)
        t.get_mut(g).unwrap().open = false;
        assert!(t.move_item(a, g, Drop::After));
        assert_eq!(names(&t)[2..], ["G", "B", "A"]);
        assert_eq!(t.get(a).unwrap().parent, None);
        // after an open group means as its first child
        t.get_mut(g).unwrap().open = true;
        assert!(t.move_item(a, g, Drop::After));
        assert_eq!(t.get(a).unwrap().parent, Some(g));
        assert_eq!(names(&t)[2..4], ["G", "A"]);
    }

    #[test]
    fn a_group_moves_with_its_layers_and_cannot_go_into_itself() {
        let (mut t, g, a, b) = tree();
        let c = t.add_map("C");
        assert_eq!(names(&t)[2..], ["C", "G", "B", "A"]);
        assert!(t.move_item(g, c, Drop::Before));
        assert_eq!(names(&t)[2..], ["G", "B", "A", "C"]);
        assert!(!t.move_item(g, g, Drop::Inside));
        assert!(!t.move_item(g, a, Drop::Before));
        assert!(!t.move_item(g, b, Drop::After));
        // a group inside a second group
        let g2 = t.add_group("G2");
        assert!(t.move_item(g, g2, Drop::Inside));
        assert_eq!(t.get(g).unwrap().parent, Some(g2));
        assert_eq!(t.rows().iter().map(|r| r.1).collect::<Vec<_>>(), vec![0, 1, 2, 2, 0]);
        // inside only works on groups
        assert!(!t.move_item(c, b, Drop::Inside));
    }

    #[test]
    fn removing_a_layer_a_group_or_a_group_with_everything_in_it() {
        let (mut t, g, a, b) = tree();
        assert_eq!(t.remove(a), vec![a]);
        assert!(t.get(a).is_none());
        // a group goes, its layer moves up
        assert!(t.remove(g).is_empty());
        assert_eq!(t.get(b).unwrap().parent, None);
        assert_eq!(names(&t), ["Specimen symbols", "Backdrop", "B"]);
        assert!(t.remove(999).is_empty());
        // a group with its layers
        let (mut t, g, a, b) = tree();
        let mut gone = t.remove_subtree(g);
        gone.sort();
        assert_eq!(gone, vec![a.min(b), a.max(b)]);
        assert_eq!(names(&t), ["Specimen symbols", "Backdrop"]);
        assert!(t.remove_subtree(g).is_empty());
    }

    #[test]
    fn the_tree_survives_the_preferences_file() {
        let (mut t, g, a, _) = tree();
        t.get_mut(a).unwrap().visible = false;
        t.get_mut(g).unwrap().open = false;
        let back = LayerTree::from_values(&t.to_values()).unwrap();
        assert_eq!(back, t);
        // new ids continue after the largest one
        let mut back = back;
        assert!(back.add_map("C") > a);
    }

    #[test]
    fn a_damaged_tree_is_refused_and_the_built_in_layers_are_pinned_at_the_front() {
        let bad = |s: &str| LayerTree::from_values(&serde_json::from_str::<Vec<Value>>(s).unwrap());
        assert!(bad(r#"[{"id":1,"kind":"map","name":"a","parent":7}]"#).is_none());
        assert!(bad(r#"[{"id":1,"kind":"map","name":"a"},{"id":2,"kind":"map","name":"b","parent":1}]"#).is_none());
        assert!(bad(r#"[{"id":1,"kind":"map","name":"a"},{"id":1,"kind":"map","name":"b"}]"#).is_none());
        assert!(bad(r#"[{"id":1,"kind":"nonsense"}]"#).is_none());
        // a child that does not sit right behind its group
        assert!(bad(r#"[{"id":1,"kind":"group","name":"g"},{"id":2,"kind":"map","name":"m"},{"id":3,"kind":"map","name":"c","parent":1}]"#).is_none());
        // missing built-in layers are added, doubled ones dropped
        let t = bad(r#"[{"id":5,"kind":"map","name":"a"}]"#).unwrap();
        assert_eq!(t.items.iter().map(|i| i.kind).collect::<Vec<_>>(), vec![Kind::Specimens, Kind::Basemap, Kind::Map]);
        let t = bad(r#"[{"id":1,"kind":"basemap","name":"x"},{"id":2,"kind":"basemap","name":"y"}]"#).unwrap();
        assert_eq!(t.items.iter().filter(|i| i.kind == Kind::Basemap).count(), 1);
        // an older file that has them in the middle (or last, or in a group): they go to the front
        let t = bad(r#"[{"id":3,"kind":"map","name":"m"},{"id":2,"kind":"basemap","name":"b"},{"id":1,"kind":"specimens","name":"s"}]"#).unwrap();
        assert_eq!(t.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(LayerTree::from_values(&[]).is_some());
    }

    #[test]
    fn the_basemap_of_older_preferences_is_called_backdrop() {
        let old = r#"[{"id":1,"kind":"specimens","name":"Specimen symbols"},{"id":2,"kind":"basemap","name":"Satellite imagery","visible":true},{"id":3,"kind":"map","name":"m"}]"#;
        let t = LayerTree::from_values(&serde_json::from_str::<Vec<Value>>(old).unwrap()).unwrap();
        assert_eq!(names(&t), ["Specimen symbols", "Backdrop", "m"]);
    }
}
