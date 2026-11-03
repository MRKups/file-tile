//! Instance-owned main-thread AppKit application menu adapter.
//!
//! Retains an instance-owned `MainThreadOnly` Objective-C menu target alongside the `NSMenu` tree.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags, NSImage,
    NSMenu, NSMenuItem,
};
use objc2_foundation::{NSData, NSInteger, NSObject, NSObjectProtocol, NSString};

const TAG_ABOUT: NSInteger = 1;
const TAG_SETTINGS: NSInteger = 2;
const TAG_BROWSE: NSInteger = 3;
const TAG_CONFIGURE: NSInteger = 4;
const TAG_LAYOUT: NSInteger = 5;
const TAG_QUIT: NSInteger = 6;
const TAG_CHECK_UPDATES: NSInteger = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    About,
    CheckForUpdates,
    Settings,
    SelectBrowse,
    SelectConfigure,
    SelectLayout,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckedMode {
    Browse,
    Configure,
    Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeMenuState {
    pub about_enabled: bool,
    pub check_updates_enabled: bool,
    pub settings_enabled: bool,
    pub modes_enabled: bool,
    pub checked_mode: CheckedMode,
}

struct MenuTargetIvars {
    on_action: Box<dyn Fn(MenuAction)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FileTileMenuTarget"]
    #[ivars = MenuTargetIvars]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(triggerMenuAction:))]
        fn trigger_menu_action(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|obj| obj.downcast_ref::<NSMenuItem>()) else {
                return;
            };
            if let Some(action) = action_from_tag(item.tag()) {
                (self.ivars().on_action)(action);
            }
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, on_action: Box<dyn Fn(MenuAction)>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MenuTargetIvars { on_action });
        // SAFETY: `NSObject::init` is safe on a freshly allocated `MenuTarget`
        // whose `MenuTargetIvars` have been initialized above.
        unsafe { msg_send![super(this), init] }
    }
}

fn action_from_tag(tag: NSInteger) -> Option<MenuAction> {
    match tag {
        TAG_ABOUT => Some(MenuAction::About),
        TAG_CHECK_UPDATES => Some(MenuAction::CheckForUpdates),
        TAG_SETTINGS => Some(MenuAction::Settings),
        TAG_BROWSE => Some(MenuAction::SelectBrowse),
        TAG_CONFIGURE => Some(MenuAction::SelectConfigure),
        TAG_LAYOUT => Some(MenuAction::SelectLayout),
        TAG_QUIT => Some(MenuAction::Quit),
        _ => None,
    }
}

/// Installs pre-rendered `.icns` (or PNG) bytes as the running `NSApplication` Dock and switcher icon.
pub fn set_application_icon(icon_bytes: &[u8]) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(icon_bytes);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: `Some(&image)` passes a valid, non-null retained `NSImage` on the main thread.
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
}

/// Main-thread AppKit menu bar handle owning the `NSMenu` hierarchy and action target.
pub struct NativeMenu {
    _main_menu: Retained<NSMenu>,
    // `NSMenuItem::target` is a zeroing weak reference in AppKit; retain `MenuTarget`
    // here so custom item targets remain alive for the lifetime of this handle.
    _target: Retained<MenuTarget>,
    about: Retained<NSMenuItem>,
    check_updates: Retained<NSMenuItem>,
    settings: Retained<NSMenuItem>,
    quit: Retained<NSMenuItem>,
    browse: Retained<NSMenuItem>,
    configure: Retained<NSMenuItem>,
    layout: Retained<NSMenuItem>,
}

impl NativeMenu {
    /// Builds and installs the `file-tile` and `View` menu bar onto `NSApplication`.
    pub fn install(on_action: impl Fn(MenuAction) + 'static) -> Result<Self, String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "application menu installation requires the main thread".to_string())?;
        let app = NSApplication::sharedApplication(mtm);
        let main_menu = new_managed_menu(mtm, "");

        let target = MenuTarget::new(mtm, Box::new(on_action));
        let app_items = build_application_submenu(mtm, &app, &target);
        main_menu.addItem(&app_items.bar_item);

        let (view_item, browse, configure, layout) = build_view_submenu(mtm, &target);
        main_menu.addItem(&view_item);

        app.setMainMenu(Some(&main_menu));
        Ok(Self {
            _main_menu: main_menu,
            _target: target,
            about: app_items.about,
            check_updates: app_items.check_updates,
            settings: app_items.settings,
            quit: app_items.quit,
            browse,
            configure,
            layout,
        })
    }

    /// Synchronizes enabled and checked state across managed `file-tile` and `View` items.
    pub fn update(&self, state: NativeMenuState) {
        self.about.setEnabled(state.about_enabled);
        self.check_updates.setEnabled(state.check_updates_enabled);
        self.settings.setEnabled(state.settings_enabled);
        for (item, mode) in [
            (&self.browse, CheckedMode::Browse),
            (&self.configure, CheckedMode::Configure),
            (&self.layout, CheckedMode::Layout),
        ] {
            item.setEnabled(state.modes_enabled);
            item.setState(if state.checked_mode == mode {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    }
}

impl Drop for NativeMenu {
    fn drop(&mut self) {
        for item in [
            &self.about,
            &self.check_updates,
            &self.settings,
            &self.quit,
            &self.browse,
            &self.configure,
            &self.layout,
        ] {
            // SAFETY: Clearing target and action to `None` detaches custom selector
            // dispatch without invoking Objective-C methods on dropped state.
            unsafe {
                item.setTarget(None);
                item.setAction(None);
            }
        }
    }
}

struct ApplicationMenuItems {
    bar_item: Retained<NSMenuItem>,
    about: Retained<NSMenuItem>,
    check_updates: Retained<NSMenuItem>,
    settings: Retained<NSMenuItem>,
    quit: Retained<NSMenuItem>,
}

fn build_application_submenu(
    mtm: MainThreadMarker,
    app: &NSApplication,
    target: &MenuTarget,
) -> ApplicationMenuItems {
    let submenu = new_managed_menu(mtm, "file-tile");
    let about = new_custom_item(mtm, "About file-tile", "", target, TAG_ABOUT, false);
    let check_updates = new_custom_item(
        mtm,
        "Check for Updates…",
        "",
        target,
        TAG_CHECK_UPDATES,
        false,
    );
    let settings = new_custom_item(mtm, "Settings…", ",", target, TAG_SETTINGS, false);
    let services = new_services_item(mtm, app);
    let hide = new_responder_item(
        mtm,
        "Hide file-tile",
        sel!(hide:),
        "h",
        NSEventModifierFlags::Command,
    );
    let hide_others = new_responder_item(
        mtm,
        "Hide Others",
        sel!(hideOtherApplications:),
        "h",
        NSEventModifierFlags::Command.union(NSEventModifierFlags::Option),
    );
    let show_all = new_responder_item(
        mtm,
        "Show All",
        sel!(unhideAllApplications:),
        "",
        NSEventModifierFlags::empty(),
    );
    let quit = new_custom_item(mtm, "Quit file-tile", "q", target, TAG_QUIT, true);

    submenu.addItem(&about);
    submenu.addItem(&check_updates);
    submenu.addItem(&NSMenuItem::separatorItem(mtm));
    submenu.addItem(&settings);
    submenu.addItem(&NSMenuItem::separatorItem(mtm));
    submenu.addItem(&services);
    submenu.addItem(&hide);
    submenu.addItem(&hide_others);
    submenu.addItem(&show_all);
    submenu.addItem(&NSMenuItem::separatorItem(mtm));
    submenu.addItem(&quit);

    let bar_item = new_submenu_holder(mtm, "file-tile", &submenu);
    ApplicationMenuItems {
        bar_item,
        about,
        check_updates,
        settings,
        quit,
    }
}

fn build_view_submenu(
    mtm: MainThreadMarker,
    target: &MenuTarget,
) -> (
    Retained<NSMenuItem>,
    Retained<NSMenuItem>,
    Retained<NSMenuItem>,
    Retained<NSMenuItem>,
) {
    let submenu = new_managed_menu(mtm, "View");
    let browse = new_custom_item(mtm, "Browse", "", target, TAG_BROWSE, false);
    browse.setState(NSControlStateValueOn);
    let configure = new_custom_item(mtm, "Configure", "", target, TAG_CONFIGURE, false);
    configure.setState(NSControlStateValueOff);
    let layout = new_custom_item(mtm, "Layout", "", target, TAG_LAYOUT, false);
    layout.setState(NSControlStateValueOff);

    submenu.addItem(&browse);
    submenu.addItem(&configure);
    submenu.addItem(&layout);

    let bar_item = new_submenu_holder(mtm, "View", &submenu);
    (bar_item, browse, configure, layout)
}

fn new_managed_menu(mtm: MainThreadMarker, title: &str) -> Retained<NSMenu> {
    let title = NSString::from_str(title);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &title);
    menu.setAutoenablesItems(false);
    menu
}

fn new_submenu_holder(
    mtm: MainThreadMarker,
    title: &str,
    submenu: &NSMenu,
) -> Retained<NSMenuItem> {
    let title = NSString::from_str(title);
    let empty = NSString::from_str("");
    // SAFETY: `action` is `None`, so no Objective-C selector is invoked on the holder item.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &title, None, &empty)
    };
    item.setSubmenu(Some(submenu));
    item.setEnabled(true);
    item
}

fn new_services_item(mtm: MainThreadMarker, app: &NSApplication) -> Retained<NSMenuItem> {
    let services_title = NSString::from_str("Services");
    // Leave `autoenablesItems` enabled on Services so AppKit manages dynamic service entries.
    let services_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &services_title);
    app.setServicesMenu(Some(&services_menu));
    new_submenu_holder(mtm, "Services", &services_menu)
}

fn new_custom_item(
    mtm: MainThreadMarker,
    title: &str,
    key: &str,
    target: &MenuTarget,
    tag: NSInteger,
    enabled: bool,
) -> Retained<NSMenuItem> {
    let title_ns = NSString::from_str(title);
    let key_ns = NSString::from_str(key);
    // SAFETY: `triggerMenuAction:` is defined on `MenuTarget` with signature `fn(&self, Option<&AnyObject>)`,
    // matching AppKit's single-sender `NSMenuItem` action contract, and `NativeMenu` retains `target`.
    let item = unsafe {
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &title_ns,
            Some(sel!(triggerMenuAction:)),
            &key_ns,
        );
        item.setTarget(Some(target));
        item
    };
    let modifiers = if key.is_empty() {
        NSEventModifierFlags::empty()
    } else {
        NSEventModifierFlags::Command
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item.setTag(tag);
    item.setEnabled(enabled);
    item
}

fn new_responder_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    key: &str,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let title_ns = NSString::from_str(title);
    let key_ns = NSString::from_str(key);
    // SAFETY: `action` is a standard `NSApplication` responder selector (`hide:`,
    // `hideOtherApplications:`, or `unhideAllApplications:`) dispatched via nil target.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &title_ns,
            Some(action),
            &key_ns,
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item.setEnabled(true);
    item
}
