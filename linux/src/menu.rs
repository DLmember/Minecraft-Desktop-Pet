//! 右键菜单：与 macOS 菜单栏等价的入口（穿透时用 Ctrl+Alt+P 打开控制台）。

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::settings;
use crate::App;

fn item(menu: &gtk::Menu, title: &str, action: impl Fn() + 'static) {
    let entry = gtk::MenuItem::with_label(title);
    entry.connect_activate(move |_| action());
    menu.append(&entry);
}

fn check_item(
    menu: &gtk::Menu,
    title: &str,
    active: bool,
    action: impl Fn(bool) + 'static,
) {
    let entry = gtk::CheckMenuItem::with_label(title);
    entry.set_active(active);
    entry.connect_toggled(move |c| action(c.is_active()));
    menu.append(&entry);
}

/// 选择 PNG 并导入（右键菜单与托盘共用）。调用方不能持有 App 的借用。
pub fn import_dialog(rc: &Rc<RefCell<App>>) {
    let parent = rc.borrow().console.as_ref().map(|c| c.window.clone());
    let dialog = gtk::FileChooserNative::new(
        Some("选择 PNG 皮肤"),
        parent.as_ref(),
        gtk::FileChooserAction::Open,
        Some("导入"),
        Some("取消"),
    );
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("PNG 图片"));
    filter.add_pattern("*.png");
    dialog.add_filter(filter);
    let response = dialog.run();
    let path = dialog.filename();
    dialog.destroy();
    if response == gtk::ResponseType::Accept {
        if let Some(path) = path {
            rc.borrow_mut().import_skin(&path);
        }
    }
}

pub fn show(rc: &Rc<RefCell<App>>) {
    let weak = Rc::downgrade(rc);
    let menu = gtk::Menu::new();

    {
        let weak = weak.clone();
        item(&menu, "打开控制台…", move || {
            if let Some(rc) = weak.upgrade() {
                let mut app = rc.borrow_mut();
                app.open_console(&rc.clone());
            }
        });
    }

    let (click_through, always_on_top, hidden) = {
        let app = rc.borrow();
        (
            settings::get_bool(&app.settings, "clickThrough"),
            settings::get_bool(&app.settings, "alwaysOnTop"),
            app.hidden,
        )
    };

    {
        let weak = weak.clone();
        check_item(&menu, "完全鼠标穿透", click_through, move |on| {
            if let Some(rc) = weak.upgrade() {
                let mut app = rc.borrow_mut();
                settings::set(&mut app.settings, "clickThrough", serde_json::json!(on));
                app.changed();
                app.save();
            }
        });
    }
    {
        let weak = weak.clone();
        check_item(&menu, "始终置顶", always_on_top, move |on| {
            if let Some(rc) = weak.upgrade() {
                let mut app = rc.borrow_mut();
                settings::set(&mut app.settings, "alwaysOnTop", serde_json::json!(on));
                app.changed();
                app.save();
            }
        });
    }

    {
        let weak = weak.clone();
        item(
            &menu,
            if hidden { "显示桌宠" } else { "隐藏桌宠" },
            move || {
                if let Some(rc) = weak.upgrade() {
                    let mut app = rc.borrow_mut();
                    let next = !app.hidden;
                    app.set_hidden(next);
                }
            },
        );
    }
    {
        let weak = weak.clone();
        item(&menu, "重新居中", move || {
            if let Some(rc) = weak.upgrade() {
                rc.borrow_mut().recenter();
            }
        });
    }

    menu.append(&gtk::SeparatorMenuItem::new());

    {
        let weak = weak.clone();
        item(&menu, "导入 PNG 皮肤…", move || {
            if let Some(rc) = weak.upgrade() {
                import_dialog(&rc);
            }
        });
    }

    menu.append(&gtk::SeparatorMenuItem::new());
    {
        let weak = weak.clone();
        item(&menu, "退出 Minecraft桌宠", move || {
            if let Some(rc) = weak.upgrade() {
                rc.borrow_mut().quit = true;
                gtk::main_quit();
            }
        });
    }

    menu.show_all();
    menu.popup_at_pointer(None);
}
