//! Presentation preferences only. Hiding a route never disables its services.
use crate::{nav, views::Route};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarPreferences {
    pub order: Vec<String>,
    pub visible: Vec<String>,
    pub start_page: String,
}
impl Default for SidebarPreferences {
    fn default() -> Self {
        Self {
            order: nav::entries().map(|e| e.id.to_owned()).collect(),
            visible: nav::entries()
                .filter(|e| e.default_visible)
                .map(|e| e.id.to_owned())
                .collect(),
            start_page: "today".into(),
        }
    }
}
impl SidebarPreferences {
    pub fn legacy(developer: bool) -> Self {
        Self {
            visible: nav::entries()
                .filter(|e| developer || e.route != Route::Developer)
                .map(|e| e.id.to_owned())
                .collect(),
            ..Self::default()
        }
    }
    pub fn ordered(&self) -> Vec<&'static nav::NavEntry> {
        let mut result = Vec::new();
        for id in &self.order {
            if let Some(entry) = nav::entries().find(|e| e.id == id)
                && !result.iter().any(|e: &&nav::NavEntry| e.id == entry.id)
            {
                result.push(entry);
            }
        }
        for entry in nav::entries() {
            if !result.iter().any(|e| e.id == entry.id) {
                result.push(entry);
            }
        }
        result
    }
    pub fn is_visible(&self, id: &str) -> bool {
        self.visible.iter().any(|v| v == id)
    }
    pub fn set_visible(&mut self, id: &str, show: bool) {
        if !nav::entries().any(|e| e.id == id) {
            return;
        }
        self.visible.retain(|v| v != id);
        if show {
            self.visible.push(id.to_owned());
        }
    }
    pub fn move_to(&mut self, id: &str, index: usize) {
        let mut order: Vec<_> = self.ordered().iter().map(|e| e.id.to_owned()).collect();
        let Some(from) = order.iter().position(|v| v == id) else {
            return;
        };
        let value = order.remove(from);
        order.insert(index.min(order.len()), value);
        self.order = order;
    }
    pub fn move_by(&mut self, id: &str, delta: isize) {
        let ordered = self.ordered();
        if let Some(index) = ordered.iter().position(|e| e.id == id) {
            self.move_to(
                id,
                index.saturating_add_signed(delta).min(ordered.len() - 1),
            );
        }
    }
    pub fn startup_route(&self) -> Route {
        nav::entries()
            .find(|e| e.id == self.start_page)
            .map_or(Route::Today, |e| e.route)
    }
    pub fn reset_sidebar(&mut self) {
        let defaults = Self::default();
        self.order = defaults.order;
        self.visible = defaults.visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_legacy_and_future_features_have_predictable_visibility() {
        let fresh = SidebarPreferences::default();
        assert_eq!(fresh.visible, ["today", "notes", "tasks", "calendar"]);
        assert_eq!(
            SidebarPreferences::legacy(true).visible.len(),
            nav::entries().count()
        );
        assert!(!SidebarPreferences::legacy(false).is_visible("developer"));
        let mut prefs = SidebarPreferences {
            order: vec!["notes".into(), "unknown".into(), "notes".into()],
            visible: vec!["notes".into()],
            start_page: "unknown".into(),
        };
        assert_eq!(prefs.ordered().len(), nav::entries().count());
        assert_eq!(prefs.ordered()[0].id, "notes");
        assert!(!prefs.is_visible("developer"));
        assert_eq!(prefs.startup_route(), Route::Today);
        prefs.move_to("calendar", 0);
        assert_eq!(prefs.ordered()[0].id, "calendar");
        prefs.move_by("calendar", 1);
        assert_eq!(prefs.ordered()[1].id, "calendar");
        prefs.visible.clear();
        assert!(prefs.ordered().iter().all(|e| !prefs.is_visible(e.id)));
        prefs.start_page = "notes".into();
        prefs.reset_sidebar();
        assert_eq!(prefs.startup_route(), Route::Notes);
    }
}
