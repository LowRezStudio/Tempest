//! The resource tree, after FFDec's: the movie, its root frames, the
//! characters by kind (linkage name or id), then the tag views (texts,
//! scripts, images, linkage, every tag). A filter matches ids, names and
//! kinds; Up/Down walk the rows; entering a clip on the stage opens its
//! group and scrolls to it. A virtual list: movies have up to ~1500 characters.

use gpui::{div, prelude::*, px, uniform_list, AnyElement, Context, ScrollStrategy, Window};
use gpui_component::input::Input;
use gpui_component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit_assets::IconName;

use super::{group_icon, MovieEditor, Node, GROUPS};

#[derive(Clone)]
pub enum Item {
    Row { node: Node, label: String, icon: IconName, extra: String, indent: bool },
    Group { id: &'static str, title: &'static str, count: usize, open: bool },
    Sep,
}

impl MovieEditor {
    fn tree_items(&self, cx: &gpui::App) -> Vec<Item> {
        let q = self.tree_filter.read(cx).value().trim().to_ascii_lowercase();
        let matches = |c: &serde_json::Value| {
            q.is_empty()
                || c["id"].as_u64().map(|i| i.to_string()) == Some(q.clone())
                || c["names"].as_array().into_iter().flatten().any(|n| n.as_str().is_some_and(|n| n.to_ascii_lowercase().contains(&q)))
                || c["kind"].as_str().is_some_and(|k| k.contains(&q))
        };
        let info = &self.info;
        let mut out = vec![
            Item::Row { node: Node::Movie, label: "Movie".into(), icon: IconName::Info, extra: format!("{} v{}", info["signature"].as_str().unwrap_or(""), info["version"]), indent: false },
            Item::Row { node: Node::Frames, label: "Frames".into(), icon: IconName::Film, extra: self.tree["rootFrames"].to_string(), indent: false },
        ];
        let chars: Vec<&serde_json::Value> = self.tree["characters"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        for (id, title, kinds) in GROUPS {
            let items: Vec<&&serde_json::Value> = chars.iter().filter(|c| c["kind"].as_str().is_some_and(|k| kinds.contains(&k)) && matches(c)).collect();
            if items.is_empty() {
                continue;
            }
            let open = !self.collapsed.contains(id) || !q.is_empty();
            out.push(Item::Group { id, title, count: items.len(), open });
            if !open {
                continue;
            }
            for c in items {
                let cid = c["id"].as_u64().unwrap_or(0);
                let name = c["names"][0].as_str();
                let label = name.map(str::to_string).unwrap_or_else(|| format!("{} {cid}", if c["kind"] == "edittext" { "EditText" } else { c["tagName"].as_str().unwrap_or("?") }));
                let extra = if name.is_some() { cid.to_string() } else if c["kind"] == "sprite" { format!("{}f", c["frames"]) } else { String::new() };
                out.push(Item::Row { node: Node::Char(cid), label, icon: group_icon(id), extra, indent: true });
            }
        }
        out.push(Item::Sep);
        let n = |k: &str| info[k].as_array().map(|a| a.len().to_string()).unwrap_or_default();
        for (node, label, icon, extra) in [
            (Node::Texts, "Edit texts", IconName::FileText, n("texts")),
            (Node::Scripts, "Scripts", IconName::Code, n("scripts")),
            (Node::Images, "External images", IconName::Image, n("images")),
            (Node::Linkage, "Linkage", IconName::Link, format!("{}/{}", n("exports"), n("imports"))),
            (Node::Tags, "All tags", IconName::ListTree, n("tags")),
        ] {
            out.push(Item::Row { node, label: label.into(), icon, extra, indent: false });
        }
        out
    }

    fn tree_key(&mut self, e: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        let k = e.keystroke.key.as_str();
        if k != "down" && k != "up" {
            return;
        }
        let items = self.tree_items(cx);
        let rows: Vec<(usize, Node)> = items.iter().enumerate().filter_map(|(i, it)| if let Item::Row { node, .. } = it { Some((i, *node)) } else { None }).collect();
        if rows.is_empty() {
            return;
        }
        let at = rows.iter().position(|(_, n)| *n == self.node);
        let next = match (at, k) {
            (None, _) => 0,
            (Some(i), "down") => (i + 1).min(rows.len() - 1),
            (Some(i), _) => i.saturating_sub(1),
        };
        self.open(rows[next].1, cx);
        self.tree_scroll.scroll_to_item(rows[next].0, ScrollStrategy::Nearest);
        cx.stop_propagation();
    }

    pub(super) fn render_tree(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        // Follow the editor: a character opened elsewhere opens its group.
        if let Node::Char(id) = self.node {
            if let Some((gid, ..)) = GROUPS.iter().find(|(_, _, kinds)| kinds.contains(&self.kind_of(id))) {
                if self.collapsed.remove(gid) {
                    cx.notify();
                }
            }
        }
        let items = self.tree_items(cx);
        if let Some(i) = items.iter().position(|it| matches!(it, Item::Row { node, .. } if *node == self.node)) {
            if self.tree_followed != Some(self.node) {
                self.tree_followed = Some(self.node);
                self.tree_scroll.scroll_to_item(i, ScrollStrategy::Nearest);
            }
        }
        self.tree_cache = items;
        let n = self.tree_cache.len();
        let list = uniform_list(
            "mv-tree",
            n,
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| {
                let theme = cx.theme().clone();
                range
                    .filter_map(|k| this.tree_cache.get(k).cloned().map(|it| (k, it)))
                    .map(|(k, it)| match it {
                        Item::Row { node, label, icon, extra, indent } => {
                            let on = this.node == node;
                            h_flex()
                                .id(("mv-tree-row", k))
                                .child(crate::driver::probe(format!("mv-tree-{}", node.key())))
                                .h(px(20.0))
                                .px_1p5()
                                .when(indent, |d| d.pl_6())
                                .gap_1p5()
                                .rounded_sm()
                                .text_xs()
                                .cursor_pointer()
                                .when(on, |d| d.bg(theme.primary).text_color(theme.primary_foreground))
                                .when(!on, |d| d.hover(|d| d.bg(theme.list_hover)))
                                .child(Icon::new(icon).xsmall())
                                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(label))
                                .when(!extra.is_empty(), |d| d.child(div().font_family("Menlo").opacity(0.5).child(extra)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.tree_focus.focus(window, cx);
                                    this.open(node, cx);
                                }))
                                // Resting on a row renders its view, so the click shows it at once.
                                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                    this.hover_seq += 1;
                                    if !*hovered {
                                        return;
                                    }
                                    let seq = this.hover_seq;
                                    cx.spawn(async move |this, cx| {
                                        cx.background_executor().timer(std::time::Duration::from_millis(80)).await;
                                        let _ = this.update(cx, |this, cx| {
                                            if this.hover_seq == seq {
                                                this.prerender(node, cx);
                                            }
                                        });
                                    })
                                    .detach();
                                }))
                                .into_any_element()
                        }
                        Item::Group { id, title, count, open } => h_flex()
                            .id(("mv-tree-group", k))
                            .h(px(20.0))
                            .px_1()
                            .gap_1()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .hover(|d| d.bg(theme.list_hover))
                            .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall())
                            .child(Icon::new(group_icon(id)).xsmall())
                            .child(title)
                            .child(div().font_weight(gpui::FontWeight::NORMAL).opacity(0.5).child(format!("({count})")))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.collapsed.remove(id) {
                                    this.collapsed.insert(id);
                                }
                                this.tree_followed = None;
                                cx.notify();
                            }))
                            .into_any_element(),
                        Item::Sep => div().h(px(20.0)).flex().items_center().child(div().h(px(1.0)).w_full().bg(theme.border)).into_any_element(),
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.tree_scroll)
        .size_full();
        v_flex()
            .size_full()
            .child(crate::driver::probe("mv-tree"))
            .child(div().p_1().child(Input::new(&self.tree_filter).xsmall()))
            .child(div().id("mv-tree-box").track_focus(&self.tree_focus).flex_1().min_h_0().px_1().pb_2().on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| this.tree_key(e, cx))).child(list))
            .border_r_1()
            .border_color(theme.border)
            .into_any_element()
    }
}
