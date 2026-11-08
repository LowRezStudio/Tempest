//! The SVG icons the app draws (Lucide, through `gpui-kit-assets`): the
//! components' own set plus the ones the Studio uses.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

gpui_kit_assets::icon_assets!(
    StudioIcons,
    [
        Package, Database, Hammer, Map, Search, Settings, FolderOpen, Folder, Save, Undo2, Keyboard, Plus, Copy, Trash, Pencil, Upload, Download, Replace, Play, Pause, Square, Box, Image, Music, FileText, Layers, ArrowLeft, ArrowRight, Clock, FilePlus, FolderPlus, Wand, Sparkles, Palette, RotateCcw, ExternalLink, CircleQuestionMark, X, Check, Film, Code, Link, PackagePlus, ListTree, Table, Volume2, Eye, EyeOff, Grid3x3, Scan, Repeat, FileArchive, Send, Info, Pin, PinOff, FolderSearch, Shirt, Store, ClipboardCopy, Briefcase, FileDown, Sun, Moon, Ellipsis, ChevronRight, ChevronDown, FileCode, SquareTerminal, Workflow, Clapperboard, MousePointer2, Move, Rotate3d, Scaling, RefreshCw, Focus, Activity, Link2, Unlink, Settings2, SkipBack
    ]
);

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(b) = StudioIcons.load(path)? {
            return Ok(Some(b));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::Assets.list(path)?;
        paths.extend(StudioIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
