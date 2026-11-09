//! Asset source: GPUI Kit's default component icons plus the Lucide icons
//! Calmfold uses.
//!
//! GPUI Kit embeds only ~100 component icons by default. Any other
//! `gpui_kit::assets::IconName` renders blank unless it is listed in
//! `AppIcons` below. When you use a new icon, add it here. The test at the
//! bottom of this file fails if a listed icon has no SVG.

use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, SharedString};

icon_assets!(
    AppIcons,
    [
        ArchiveRestore,
        Cake,
        Gift,
        Heading,
        ArrowLeft,
        FolderOutput,
        FolderInput,
        Quote,
        ListOrdered,
        List,
        Pilcrow,
        Heading3,
        Heading2,
        Heading1,
        Code,
        Highlighter,
        Strikethrough,
        Italic,
        Bold,
        Type,
        Pencil,
        Eye,
        FolderPlus,
        ListChecks,
        Upload,
        Download,
        NotebookTabs,
        CalendarRange,
        FolderKanban,
        Repeat,
        X,
        Hash,
        RotateCcw,
        Play,
        Pause,
        PanelRightOpen,
        Ban,
        CalendarCheck,
        CalendarClock,
        CalendarDays,
        CalendarPlus,
        Archive,
        Check,
        Flame,
        ShieldCheck,
        Wifi,
        BadgeCheck,
        Terminal,
        IdCard,
        KeyRound,
        LockKeyhole,
        CreditCard,
        StickyNote,
        Star,
        Copy,
        EyeOff,
        ExternalLink,
        Dices,
        RefreshCw,
        Sparkles,
        ChevronRight,
        SlidersHorizontal,
        ChartColumn,
        TrendingUp,
        TrendingDown,
        Sprout,
        ChevronLeft,
        Circle,
        MapPin,
        CircleCheckBig,
        Ellipsis,
        FileText,
        Flag,
        Gem,
        HardDrive,
        House,
        Info,
        Keyboard,
        Link2,
        ListTodo,
        Minus,
        Moon,
        NotebookPen,
        NotebookText,
        Palette,
        PanelLeft,
        PanelLeftDashed,
        Pin,
        PinOff,
        Plus,
        Search,
        ChevronUp,
        ChevronDown,
        Settings,
        SquarePen,
        Sun,
        SunMoon,
        Sunrise,
        Trash,
        TriangleAlert,
    ]
);

/// Pass to `gpui_kit::application().with_assets(...)`.
pub struct CalmfoldAssets;

impl AssetSource for CalmfoldAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = AppIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(AppIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::assets::IconName;
    use std::path::Path;

    fn collect_used(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "target") {
                    collect_used(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for part in text.split("Lucide::").skip(1) {
                    let name: String = part
                        .chars()
                        .take_while(char::is_ascii_alphanumeric)
                        .collect();
                    if !name.is_empty() && !out.contains(&name) {
                        out.push(name);
                    }
                }
            }
        }
    }

    /// Every Lucide icon referenced anywhere in the workspace must be embedded,
    /// otherwise it silently renders blank.
    #[test]
    fn every_used_icon_is_embedded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut used = Vec::new();
        collect_used(&root.join("apps"), &mut used);
        collect_used(&root.join("crates"), &mut used);
        assert!(!used.is_empty(), "scan found icon uses");

        let missing: Vec<_> = used
            .iter()
            .filter(|name| {
                let icon = IconName::ALL.iter().find(|i| format!("{i:?}") == **name);
                let Some(icon) = icon else { return true };
                !matches!(
                    CalmfoldAssets.load(&gpui_kit::assets::IconNamed::path(*icon)),
                    Ok(Some(_))
                )
            })
            .collect();
        assert!(
            missing.is_empty(),
            "add these to AppIcons in crates/ui/src/assets.rs: {missing:?}"
        );
    }
}
