//! A cleanup plan: the exact list of files a scan proposes, each with its size and the reason.
//! Nothing is removed except the selected items of a plan the user confirmed, and the
//! confirmation names the plan's digest, so a list that changed after it was shown is refused.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fsutil::human_size;

/// Every CleanCraft feature. The id strings are used by the CLI, MCP tools and settings.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FeatureId {
    SmartCare,
    SystemJunk,
    MailAttachments,
    TrashBins,
    LargeOldFiles,
    Duplicates,
    SimilarPhotos,
    Uninstaller,
    Updater,
    LoginItems,
    Maintenance,
    Malware,
    Privacy,
    SpaceLens,
    Monitor,
}

impl FeatureId {
    pub const ALL: [FeatureId; 15] = [
        FeatureId::SmartCare,
        FeatureId::SystemJunk,
        FeatureId::MailAttachments,
        FeatureId::TrashBins,
        FeatureId::LargeOldFiles,
        FeatureId::Duplicates,
        FeatureId::SimilarPhotos,
        FeatureId::Uninstaller,
        FeatureId::Updater,
        FeatureId::LoginItems,
        FeatureId::Maintenance,
        FeatureId::Malware,
        FeatureId::Privacy,
        FeatureId::SpaceLens,
        FeatureId::Monitor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FeatureId::SmartCare => "smart_care",
            FeatureId::SystemJunk => "system_junk",
            FeatureId::MailAttachments => "mail_attachments",
            FeatureId::TrashBins => "trash_bins",
            FeatureId::LargeOldFiles => "large_old_files",
            FeatureId::Duplicates => "duplicates",
            FeatureId::SimilarPhotos => "similar_photos",
            FeatureId::Uninstaller => "uninstaller",
            FeatureId::Updater => "updater",
            FeatureId::LoginItems => "login_items",
            FeatureId::Maintenance => "maintenance",
            FeatureId::Malware => "malware",
            FeatureId::Privacy => "privacy",
            FeatureId::SpaceLens => "space_lens",
            FeatureId::Monitor => "monitor",
        }
    }

    pub fn parse(s: &str) -> Option<FeatureId> {
        let s = s.trim().replace('-', "_").to_lowercase();
        FeatureId::ALL.into_iter().find(|f| f.as_str() == s)
    }

    pub fn title(self) -> &'static str {
        match self {
            FeatureId::SmartCare => "Quick Sweep",
            FeatureId::SystemJunk => "System Junk",
            FeatureId::MailAttachments => "Mail Downloads",
            FeatureId::TrashBins => "Trash Bins",
            FeatureId::LargeOldFiles => "Large & Old Files",
            FeatureId::Duplicates => "Duplicates",
            FeatureId::SimilarPhotos => "Similar Images",
            FeatureId::Uninstaller => "App Remover",
            FeatureId::Updater => "App Updates",
            FeatureId::LoginItems => "Startup Items",
            FeatureId::Maintenance => "Tune-up Tasks",
            FeatureId::Malware => "Threat Check",
            FeatureId::Privacy => "Privacy Traces",
            FeatureId::SpaceLens => "Disk Map",
            FeatureId::Monitor => "Live Monitor",
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Rebuilt automatically by macOS or the app (caches, logs).
    Low,
    /// Safe for most people, but something is lost (history, downloaded attachments).
    Medium,
    /// The user's own files or app parts; selected only by hand.
    High,
}

impl Risk {
    pub fn label(self) -> &'static str {
        match self {
            Risk::Low => "Low",
            Risk::Medium => "Medium",
            Risk::High => "High",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Category {
    pub id: String,
    pub title: String,
    pub description: String,
    pub risk: Risk,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Item {
    /// Index in the plan, stable for the plan's life.
    pub id: usize,
    pub path: PathBuf,
    /// Bytes on disk, same formula as [`crate::fsutil::tree_size`].
    pub size: u64,
    pub category: String,
    /// Why it is proposed, in words the user understands.
    pub reason: String,
    pub risk: Risk,
    pub selected: bool,
    /// When set, the item is shown but cannot be selected, and this says why.
    pub locked: Option<String>,
    /// Optional group key: duplicates and similar images share one per set.
    #[serde(default)]
    pub group: Option<String>,
    /// Seconds since the epoch, when known.
    #[serde(default)]
    pub modified: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Plan {
    pub id: String,
    pub feature: Option<FeatureId>,
    pub created: i64,
    pub categories: Vec<Category>,
    pub items: Vec<Item>,
    /// Things the user should know: locations that could not be read, missing permissions.
    pub notes: Vec<String>,
}

impl Plan {
    pub fn new(feature: FeatureId) -> Plan {
        let now = chrono::Utc::now();
        Plan {
            id: format!("{}-{}", feature.as_str(), now.format("%Y%m%d%H%M%S%3f")),
            feature: Some(feature),
            created: now.timestamp(),
            ..Default::default()
        }
    }

    pub fn add_category(&mut self, id: &str, title: &str, description: &str, risk: Risk) {
        if !self.categories.iter().any(|c| c.id == id) {
            self.categories.push(Category {
                id: id.into(),
                title: title.into(),
                description: description.into(),
                risk,
            });
        }
    }

    /// Add an item; `selected` is ignored when `locked` is set.
    pub fn push(
        &mut self,
        path: PathBuf,
        size: u64,
        category: &str,
        reason: impl Into<String>,
        risk: Risk,
        selected: bool,
    ) -> usize {
        let id = self.items.len();
        self.items.push(Item {
            id,
            path,
            size,
            category: category.into(),
            reason: reason.into(),
            risk,
            selected,
            locked: None,
            group: None,
            modified: None,
        });
        id
    }

    pub fn lock(&mut self, id: usize, why: impl Into<String>) {
        if let Some(it) = self.items.get_mut(id) {
            it.locked = Some(why.into());
            it.selected = false;
        }
    }

    pub fn set_selected(&mut self, id: usize, on: bool) {
        if let Some(it) = self.items.get_mut(id)
            && it.locked.is_none()
        {
            it.selected = on;
        }
    }

    pub fn set_category_selected(&mut self, cat: &str, on: bool) {
        for it in self
            .items
            .iter_mut()
            .filter(|i| i.category == cat && i.locked.is_none())
        {
            it.selected = on;
        }
    }

    pub fn selected(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(|i| i.selected && i.locked.is_none())
    }

    pub fn total_size(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }

    pub fn selected_size(&self) -> u64 {
        self.selected().map(|i| i.size).sum()
    }

    pub fn selected_count(&self) -> usize {
        self.selected().count()
    }

    pub fn category_size(&self, cat: &str) -> u64 {
        self.items
            .iter()
            .filter(|i| i.category == cat)
            .map(|i| i.size)
            .sum()
    }

    pub fn category_count(&self, cat: &str) -> usize {
        self.items.iter().filter(|i| i.category == cat).count()
    }

    /// Fingerprint of exactly what would be removed: plan id plus each selected path and size.
    /// The user's confirmation carries it; [`crate::executor::execute`] refuses a mismatch.
    pub fn digest(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.id.as_bytes());
        for it in self.selected() {
            h.update([0]);
            h.update(it.path.to_string_lossy().as_bytes());
            h.update([0]);
            h.update(it.size.to_le_bytes());
        }
        hex::encode(h.finalize())
    }

    pub fn summary(&self) -> String {
        format!(
            "{} items, {} selected ({})",
            self.items.len(),
            self.selected_count(),
            human_size(self.selected_size())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for f in FeatureId::ALL {
            assert_eq!(FeatureId::parse(f.as_str()), Some(f));
        }
        assert_eq!(FeatureId::parse("system-junk"), Some(FeatureId::SystemJunk));
    }

    #[test]
    fn digest_follows_selection() {
        let mut p = Plan::new(FeatureId::SystemJunk);
        p.push("/a".into(), 10, "c", "r", Risk::Low, true);
        p.push("/b".into(), 20, "c", "r", Risk::Low, true);
        let d1 = p.digest();
        p.set_selected(1, false);
        assert_ne!(d1, p.digest());
        assert_eq!(p.selected_size(), 10);
        p.lock(0, "in use");
        assert_eq!(p.selected_count(), 0);
        p.set_selected(0, true);
        assert_eq!(p.selected_count(), 0, "locked items cannot be selected");
    }
}
