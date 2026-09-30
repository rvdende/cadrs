//! **Items** (P3B.8, `intro-to-assemblies.md` A1.7, X16): the Instances list's "Items (n)"
//! group, between the instances and the mates: things the assembly needs that have no geometry
//! (glue, grease, a label, a purchased item not modelled). Each has a name, a quantity and the
//! usual [`crate::properties`] (Part number, Description, …, [`crate::properties::PropertyOwner::Item`]),
//! and is a row of the assembly's Bill of Materials ([`super::bom`]).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::commands::assembly_mut;
use super::Assembly;
use crate::command::{Command, CommandError, Scope};
use crate::document::Document;
use crate::ids::ElementId;
use crate::properties::Properties;

/// Identifies an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemId(pub Uuid);

impl ItemId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_u128(v: u128) -> Self {
        Self(Uuid::from_u128(v))
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

/// A non-geometric item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: ItemId,
    pub name: String,
    /// How many the assembly needs (the BOM's Quantity).
    pub quantity: u32,
    #[serde(default, skip_serializing_if = "Properties::is_empty")]
    pub properties: Properties,
}

impl Item {
    pub fn new(id: ItemId, name: impl Into<String>, quantity: u32) -> Self {
        Self { id, name: name.into(), quantity, properties: Properties::default() }
    }
}

impl Assembly {
    pub fn item(&self, id: ItemId) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_mut(&mut self, id: ItemId) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }
}

/// Adds or replaces an item (the Add item dialog, Edit): one undo step.
#[derive(Debug, Clone)]
pub struct SetItem {
    pub element: ElementId,
    pub item: Item,
}

impl Command for SetItem {
    fn label(&self) -> String {
        format!("Item: {}", self.item.name)
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.item.name.trim().is_empty() {
            return Err(CommandError::Invalid("an item needs a name".into()));
        }
        if self.item.quantity == 0 {
            return Err(CommandError::Invalid("the quantity must be at least 1".into()));
        }
        let asm = assembly_mut(doc, self.element)?;
        let mut item = self.item.clone();
        item.name = item.name.trim().to_string();
        match asm.item_mut(item.id) {
            Some(slot) => *slot = item,
            None => asm.items.push(item),
        }
        Ok(())
    }
}

/// Deletes items.
#[derive(Debug, Clone)]
pub struct DeleteItems {
    pub element: ElementId,
    pub items: Vec<ItemId>,
}

impl Command for DeleteItems {
    fn label(&self) -> String {
        "Delete items".into()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        if self.items.is_empty() || self.items.iter().any(|i| asm.item(*i).is_none()) {
            return Err(CommandError::Invalid("item not found".into()));
        }
        asm.items.retain(|i| !self.items.contains(&i.id));
        Ok(())
    }
}
